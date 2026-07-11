//! WGPU renderer with a SortedAlpha reference path.

mod packed_atlas;
mod packed_gpu;
mod page_atlas;
mod page_scheduler;
mod residency;
mod spatial_pages;
mod surface_session;

pub use packed_atlas::{
    DEGREE3_SIDECAR_BYTES, DIRECT_DEGREE3_ATTRIBUTE_BYTES, FULL_DEGREE3_ATTRIBUTE_BYTES,
    HOT_RECORD_BYTES, HotStream, LogScaleRange, PackedAtlasCpuBuffers, PackedHotRecord,
    PackedSceneCpu, PackedShSidecar, SceneBounds, atlas_dimensions, decode_opacity_u8,
    dequantize_sh_rest, measured_hot_texture_bytes, measured_sh_sidecar_texture_bytes,
    pack_color_rgb10, pack_quat_smallest_three, pack_scene, sh_sidecar_atlas_dimensions,
    slot_to_texel, unpack_color_rgb10, unpack_quat_smallest_three,
};
pub use page_atlas::{
    PageAtlasCpu, PageAtlasError, PageAtlasSlotCpu, attribute_bytes_for_lod, extract_page_scene,
};
pub use page_scheduler::{ScheduleOutcome, SchedulerConfig, SchedulerView, schedule_pages};
pub use residency::{
    AsyncPageToken, AtlasSlot, AttributeLod, PageResidency, PageResidencyState, ResidencyBudgets,
    ResidencyError, ResidencyManager,
};
pub use spatial_pages::{
    DEFAULT_PAGE_CAPACITY, PageBounds, PageId, SpatialPage, SpatialPageSet, partition_scene_pages,
};
pub use surface_session::{
    SurfaceFrameOutput, SurfaceFrameTimings, SurfaceRenderSession, SurfaceSortSchedule,
};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_sort::{CpuSortBackend, SortError};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[cfg(not(target_arch = "wasm32"))]
const RENDER_TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[cfg(not(target_arch = "wasm32"))]
type TimerInstant = Instant;

#[cfg(target_arch = "wasm32")]
type TimerInstant = f64;

#[cfg(not(target_arch = "wasm32"))]
fn timer_now() -> TimerInstant {
    Instant::now()
}

#[cfg(target_arch = "wasm32")]
fn timer_now() -> TimerInstant {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
fn timer_elapsed_ms(start: TimerInstant) -> f32 {
    start.elapsed().as_secs_f32() * 1000.0
}

#[cfg(target_arch = "wasm32")]
fn timer_elapsed_ms(start: TimerInstant) -> f32 {
    (js_sys::Date::now() - start).max(0.0) as f32
}

#[cfg(target_os = "android")]
pub(crate) const fn wgpu_label(_label: &'static str) -> Option<&'static str> {
    None
}

#[cfg(not(target_os = "android"))]
pub(crate) const fn wgpu_label(label: &'static str) -> Option<&'static str> {
    Some(label)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeometryPath {
    #[default]
    SortedIndexDirect,
    PackedAtlas,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOutput {
    pub depth_keys: Vec<u32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("invalid renderer configuration")]
    InvalidConfig,
    #[error("invalid camera")]
    InvalidCamera,
    #[error("scene not loaded")]
    SceneNotLoaded,
    #[error("invalid scene buffers")]
    InvalidScene,
    #[error("gpu rasterizer unavailable")]
    GpuRasterizerUnavailable,
    #[error("gpu device creation failed")]
    GpuDeviceCreation,
    #[error(
        "render dimensions {width}x{height} exceed the device 2D texture limit {max_dimension}"
    )]
    GpuDimensionsUnsupported {
        width: u32,
        height: u32,
        max_dimension: u32,
    },
    #[error("gpu readback failed")]
    GpuReadback,
    #[error("waiting for gpu completion failed")]
    GpuWait,
    #[error("surface background worker failed")]
    SurfaceWorker,
    #[error("direct scene resource error: {0}")]
    DirectScene(#[from] DirectSceneError),
    #[error("sort backend error: {0}")]
    Sort(#[from] SortError),
    #[error("surface presenter error: {0}")]
    SurfacePresenter(#[from] SurfacePresenterError),
}

impl RendererError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidConfig | Self::InvalidCamera | Self::InvalidScene => {
                ErrorCode::InvalidArgument
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::GpuRasterizerUnavailable
            | Self::GpuDeviceCreation
            | Self::GpuDimensionsUnsupported { .. }
            | Self::DirectScene(DirectSceneError::ResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::PackedResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::ResourceSizeOverflow) => ErrorCode::Unsupported,
            Self::GpuReadback | Self::GpuWait | Self::SurfaceWorker => ErrorCode::Internal,
            Self::DirectScene(DirectSceneError::SortedIndexCapacityExceeded) => ErrorCode::Internal,
            Self::Sort(_) => ErrorCode::Internal,
            Self::SurfacePresenter(err) => err.code(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSceneResource {
    SortedIndices,
    Source,
    ShRest,
    DrawInstances,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectSceneResourceRequirement {
    pub resource: DirectSceneResource,
    pub required_bytes: u64,
    pub limit_bytes: u64,
    pub fits: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectScenePath {
    Direct,
    ActiveAtlasRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSceneRemediation {
    None,
    UseActiveAtlasOrReduce { max_direct_splats: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectScenePreflight {
    pub splat_count: u64,
    pub sh_degree: u8,
    pub path: DirectScenePath,
    pub effective_storage_binding_limit: u64,
    pub effective_max_buffer_size: u64,
    pub requirements: [DirectSceneResourceRequirement; 3],
    pub limiting_resource: DirectSceneResource,
    pub max_direct_splats: u64,
    pub remediation: DirectSceneRemediation,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DirectSceneError {
    #[error("sorted index buffer capacity exceeded")]
    SortedIndexCapacityExceeded,
    #[error("direct scene resource size overflow")]
    ResourceSizeOverflow,
    #[error("direct scene resources exceed effective device limits: {0:?}")]
    ResourceLimitExceeded(Box<DirectScenePreflight>),
    #[error("packed scene resources require paging or exceed effective device limits: {0:?}")]
    PackedResourceLimitExceeded(Box<PackedScenePreflight>),
}

#[derive(Debug, Error)]
pub enum SurfacePresenterError {
    #[error("invalid surface size")]
    InvalidSurfaceSize,
    #[error("surface creation failed")]
    SurfaceCreation,
    #[error("no compatible surface adapter")]
    NoAdapter,
    #[error("surface device creation failed: {0}")]
    DeviceCreation(String),
    #[error("surface has no compatible format")]
    NoSurfaceFormat,
    #[error("surface configure failed: {0}")]
    SurfaceConfigure(String),
    #[error("surface requires a loaded scene")]
    SceneNotLoaded,
    #[error("surface acquire failed: {0}")]
    SurfaceAcquire(String),
    #[error("surface out of memory")]
    SurfaceOutOfMemory,
    #[error("direct scene resource error: {0}")]
    DirectScene(#[from] DirectSceneError),
}

impl SurfacePresenterError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidSurfaceSize => ErrorCode::InvalidArgument,
            Self::SurfaceCreation | Self::NoAdapter | Self::DeviceCreation(_) => {
                ErrorCode::Unsupported
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::NoSurfaceFormat
            | Self::SurfaceConfigure(_)
            | Self::SurfaceAcquire(_)
            | Self::SurfaceOutOfMemory => ErrorCode::Internal,
            Self::DirectScene(DirectSceneError::ResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::PackedResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::ResourceSizeOverflow) => ErrorCode::Unsupported,
            Self::DirectScene(DirectSceneError::SortedIndexCapacityExceeded) => ErrorCode::Internal,
        }
    }
}

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    geometry_path: GeometryPath,
    cpu_sort_backend: CpuSortBackend,
    #[cfg(not(target_arch = "wasm32"))]
    gpu_rasterizer: Option<GpuRasterizer>,
    scene: Option<SceneBuffers>,
    world_covariances: Option<Vec<[[f32; 3]; 3]>>,
    world_covariance_terms: Option<Vec<CameraCovarianceTerms>>,
    alpha_values: Option<Vec<f32>>,
    preprocess_depth_keys: Vec<u32>,
    preprocess_indices: Vec<u32>,
    last_stats: FrameStats,
}

impl Renderer {
    pub fn new(mode: RenderMode) -> Result<Self, RendererError> {
        let config = RendererConfig {
            mode,
            ..RendererConfig::default()
        };
        Self::with_config(config)
    }

    pub fn with_config(config: RendererConfig) -> Result<Self, RendererError> {
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let gpu_rasterizer = GpuRasterizer::create(&config).map_err(RendererError::from)?;
            let mut renderer = Self::from_validated_config(config);
            renderer.gpu_rasterizer = Some(gpu_rasterizer);
            Ok(renderer)
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    /// Create renderer state for a separate native or Web surface presenter.
    ///
    /// This constructor intentionally does not create the offscreen rasterizer
    /// used by [`Self::render_frame`] and [`Self::readback_rgba8`]. Surface
    /// clients render through [`SurfacePresenter`] instead.
    pub fn with_config_for_surface(config: RendererConfig) -> Result<Self, RendererError> {
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;
        Ok(Self::from_validated_config(config))
    }

    pub fn new_for_surface(mode: RenderMode) -> Result<Self, RendererError> {
        Self::with_config_for_surface(RendererConfig {
            mode,
            ..RendererConfig::default()
        })
    }

    fn from_validated_config(config: RendererConfig) -> Self {
        Self {
            mode: config.mode,
            config,
            geometry_path: GeometryPath::SortedIndexDirect,
            cpu_sort_backend: CpuSortBackend::default(),
            #[cfg(not(target_arch = "wasm32"))]
            gpu_rasterizer: None,
            scene: None,
            world_covariances: None,
            world_covariance_terms: None,
            alpha_values: None,
            preprocess_depth_keys: Vec::new(),
            preprocess_indices: Vec::new(),
            last_stats: FrameStats::zero(),
        }
    }

    pub fn config(&self) -> RendererConfig {
        self.config
    }

    pub fn geometry_path(&self) -> GeometryPath {
        self.geometry_path
    }

    pub fn set_geometry_path(&mut self, path: GeometryPath) {
        if self.geometry_path != path {
            self.geometry_path = path;
            self.rebuild_path_specific_cpu_data();
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
                rasterizer.clear_scene_resources();
            }
        }
    }

    pub fn set_size(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let config = RendererConfig {
            width,
            height,
            ..self.config
        };
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(gpu_rasterizer) = self.gpu_rasterizer.as_mut() {
            gpu_rasterizer
                .ensure_output_target(width, height)
                .map_err(RendererError::from)?;
        }

        self.config = config;
        Ok(())
    }

    pub fn mode(&self) -> RenderMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: RenderMode) {
        self.mode = mode;
        self.config.mode = mode;
    }

    pub fn has_gpu_rasterizer(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.gpu_rasterizer.is_some()
        }

        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn gpu_adapter_info(&self) -> Option<&wgpu::AdapterInfo> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.gpu_rasterizer
                .as_ref()
                .map(|rasterizer| &rasterizer.adapter_info)
        }

        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Reports whether the loaded scene fits the Direct path on this renderer's
    /// effective offscreen device limits.
    ///
    /// Surface-only renderers do not own a device, so callers must query the
    /// presenter path separately instead of assuming adapter or default limits.
    pub fn current_direct_scene_preflight(&self) -> Result<DirectScenePreflight, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            direct_scene_preflight(scene.len(), scene.sh_degree, &rasterizer.device.limits())
                .map_err(RendererError::from)
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = scene;
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn wait_for_gpu(&self) -> Result<(), RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            rasterizer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| RendererError::GpuWait)?;
            Ok(())
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn load_scene(&mut self, scene: SceneBuffers) -> Result<(), RendererError> {
        scene.validate().map_err(|_| RendererError::InvalidScene)?;
        self.scene = Some(scene);
        self.rebuild_path_specific_cpu_data();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
            rasterizer.clear_scene_resources();
        }
        Ok(())
    }

    fn rebuild_path_specific_cpu_data(&mut self) {
        match (self.geometry_path, self.scene.as_ref()) {
            (GeometryPath::SortedIndexDirect, Some(scene)) => {
                let world_covariances = precompute_world_covariances(scene);
                let world_covariance_terms = world_covariances
                    .iter()
                    .copied()
                    .map(CameraCovarianceTerms::from_matrix)
                    .collect();
                let alpha_values = precompute_alpha_values(scene);
                self.world_covariances = Some(world_covariances);
                self.world_covariance_terms = Some(world_covariance_terms);
                self.alpha_values = Some(alpha_values);
            }
            _ => {
                self.world_covariances = None;
                self.world_covariance_terms = None;
                self.alpha_values = None;
            }
        }
    }

    pub fn scene(&self) -> Option<&SceneBuffers> {
        self.scene.as_ref()
    }

    pub fn world_covariances(&self) -> Option<&[[[f32; 3]; 3]]> {
        self.world_covariances.as_deref()
    }

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let mut output = PreprocessOutput {
            depth_keys: Vec::with_capacity(scene.len()),
            indices: Vec::with_capacity(scene.len()),
        };
        preprocess_visible_into(scene, camera, &mut output.depth_keys, &mut output.indices)?;
        Ok(output)
    }

    pub fn build_sorted_instances(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<GpuInstance>, FrameStats), RendererError> {
        let mut instances = Vec::new();
        let stats = self.build_sorted_instances_into(camera, &mut instances)?;
        Ok((instances, stats))
    }

    pub fn build_sorted_instances_into(
        &mut self,
        camera: &Camera,
        instances: &mut Vec<GpuInstance>,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let preprocess_start = timer_now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = timer_elapsed_ms(preprocess_start);

        let sort_start = timer_now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = timer_elapsed_ms(sort_start);

        let raster_start = timer_now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let alpha_values = self
            .alpha_values
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        build_instances_into(
            scene,
            world_covariances,
            alpha_values,
            &self.preprocess_indices,
            camera,
            self.config,
            instances,
        );
        let drawn_count = instances.len() as u32;
        let raster_ms = timer_elapsed_ms(raster_start);

        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: self.preprocess_indices.len() as u32,
            drawn_count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    pub fn build_surface_sorted_indices_with_sort_refresh(
        &mut self,
        camera: &Camera,
        refresh_sort: bool,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let refresh_sort = refresh_sort || self.preprocess_indices.is_empty();
        let (preprocess_ms, sort_ms) = if refresh_sort {
            let preprocess_start = timer_now();
            self.preprocess_visible_scratch(camera)?;
            let preprocess_ms = timer_elapsed_ms(preprocess_start);

            let sort_start = timer_now();
            self.sort_preprocessed_scratch()?;
            let sort_ms = timer_elapsed_ms(sort_start);

            (preprocess_ms, sort_ms)
        } else {
            camera
                .validate()
                .map_err(|_| RendererError::InvalidCamera)?;
            (0.0, 0.0)
        };

        let visible_count = self.preprocess_indices.len() as u32;
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms: 0.0,
            visible_count,
            drawn_count: visible_count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    pub fn current_sorted_indices(&self) -> &[u32] {
        &self.preprocess_indices
    }

    pub fn replace_surface_sorted_indices(
        &mut self,
        indices: Vec<u32>,
    ) -> Result<(), RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        if indices.iter().any(|&idx| idx as usize >= scene.len()) {
            return Err(RendererError::InvalidScene);
        }

        self.preprocess_depth_keys.clear();
        self.preprocess_indices = indices;
        Ok(())
    }

    pub fn build_sorted_indices(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<u32>, FrameStats), RendererError> {
        let frame_start = timer_now();

        let preprocess_start = timer_now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = timer_elapsed_ms(preprocess_start);

        let sort_start = timer_now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = timer_elapsed_ms(sort_start);

        let visible_count = self.preprocess_indices.len() as u32;
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms: 0.0,
            visible_count,
            drawn_count: visible_count,
        };
        self.last_stats = stats;
        Ok((self.preprocess_indices.clone(), stats))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        if self.gpu_rasterizer.is_none() {
            return Err(RendererError::GpuRasterizerUnavailable);
        }

        let frame_start = timer_now();

        let preprocess_start = timer_now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = timer_elapsed_ms(preprocess_start);

        let sort_start = timer_now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = timer_elapsed_ms(sort_start);

        let raster_start = timer_now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let sorted_indices = &self.preprocess_indices;
        match self.geometry_path {
            GeometryPath::SortedIndexDirect => {
                let world_covariance_terms = self
                    .world_covariance_terms
                    .as_deref()
                    .ok_or(RendererError::InvalidScene)?;
                let alpha_values = self
                    .alpha_values
                    .as_deref()
                    .ok_or(RendererError::InvalidScene)?;
                self.gpu_rasterizer
                    .as_mut()
                    .ok_or(RendererError::GpuRasterizerUnavailable)?
                    .render_direct_sorted_indices(
                        self.config,
                        sorted_indices,
                        camera,
                        scene,
                        world_covariance_terms,
                        alpha_values,
                    )
                    .map_err(RendererError::from)?;
            }
            GeometryPath::PackedAtlas => {
                self.gpu_rasterizer
                    .as_mut()
                    .ok_or(RendererError::GpuRasterizerUnavailable)?
                    .render_packed_sorted_indices(self.config, sorted_indices, camera, scene)
                    .map_err(RendererError::from)?;
            }
        }
        let drawn_count = sorted_indices.len() as u32;
        let raster_ms = timer_elapsed_ms(raster_start);

        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: self.preprocess_indices.len() as u32,
            drawn_count,
        };

        self.last_stats = stats;
        Ok(stats)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn render_frame_with_external_order_for_test(
        &mut self,
        camera: &Camera,
        sorted_indices: &[u32],
    ) -> Result<FrameStats, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        let frame_start = timer_now();
        let raster_start = timer_now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        match self.geometry_path {
            GeometryPath::SortedIndexDirect => {
                let world_covariance_terms = self
                    .world_covariance_terms
                    .as_deref()
                    .ok_or(RendererError::InvalidScene)?;
                let alpha_values = self
                    .alpha_values
                    .as_deref()
                    .ok_or(RendererError::InvalidScene)?;
                self.gpu_rasterizer
                    .as_mut()
                    .ok_or(RendererError::GpuRasterizerUnavailable)?
                    .render_direct_sorted_indices(
                        self.config,
                        sorted_indices,
                        camera,
                        scene,
                        world_covariance_terms,
                        alpha_values,
                    )
                    .map_err(RendererError::from)?;
            }
            GeometryPath::PackedAtlas => {
                self.gpu_rasterizer
                    .as_mut()
                    .ok_or(RendererError::GpuRasterizerUnavailable)?
                    .render_packed_sorted_indices(self.config, sorted_indices, camera, scene)
                    .map_err(RendererError::from)?;
            }
        }
        let raster_ms = timer_elapsed_ms(raster_start);
        let count = u32::try_from(sorted_indices.len()).unwrap_or(u32::MAX);
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms,
            visible_count: count,
            drawn_count: count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn render_frame(&mut self, _camera: &Camera) -> Result<FrameStats, RendererError> {
        Err(RendererError::GpuRasterizerUnavailable)
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub fn readback_rgba8(&mut self) -> Result<Vec<u8>, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            rasterizer
                .readback_rgba8()
                .map_err(|_| RendererError::GpuReadback)
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn render_placeholder(&mut self) -> Result<FrameStats, RendererError> {
        self.render_frame(&Camera::default())
    }

    fn preprocess_visible_scratch(&mut self, camera: &Camera) -> Result<(), RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        preprocess_visible_into(
            scene,
            camera,
            &mut self.preprocess_depth_keys,
            &mut self.preprocess_indices,
        )
    }

    fn sort_preprocessed_scratch(&mut self) -> Result<(), RendererError> {
        if self.mode == RenderMode::SortedAlpha {
            self.cpu_sort_backend
                .sort_values_by_keys(&self.preprocess_depth_keys, &mut self.preprocess_indices)?;
        }
        Ok(())
    }
}

pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    direct_pipeline: wgpu::RenderPipeline,
    direct_bind_group_layout: wgpu::BindGroupLayout,
    packed_pipeline: wgpu::RenderPipeline,
    packed_bind_group_layout: wgpu::BindGroupLayout,
    surface_config: wgpu::SurfaceConfiguration,
    max_texture_dimension_2d: u32,
    instance_count: u32,
    geometry_path: GeometryPath,
    direct_scene: Option<DirectSceneResources>,
    packed_scene: Option<packed_gpu::PackedAtlasResources>,
    /// Last camera position whose hot colors have been fully applied.
    packed_color_refresh_position: Option<[f32; 3]>,
    /// Next splat index for an in-flight banded refresh, if any.
    packed_color_refresh_cursor: Option<usize>,
    /// Camera position the in-flight banded refresh is converging toward.
    packed_color_refresh_target: Option<[f32; 3]>,
}

impl SurfacePresenter {
    /// Creates a presenter for an owned native window target.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_window<T>(
        target: T,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let instance = create_surface_instance();
        let surface = instance
            .create_surface(target)
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    /// Creates a presenter from raw handles supplied by an embedding platform.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the raw display and window handles remain valid until
    /// after the returned presenter is dropped.
    pub unsafe fn from_raw_handles(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        pollster::block_on(Self::from_raw_handles_async(
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            renderer,
        ))
    }

    async fn from_raw_handles_async(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let instance = create_surface_instance();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|_| SurfacePresenterError::SurfaceCreation)?;

        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let instance = create_surface_instance();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;

        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    async fn from_surface_async(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| SurfacePresenterError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        let adapter_limits = adapter.limits();
        let mut required_limits = wgpu::Limits::downlevel_defaults();
        let mut required_texture_dimension = width.max(height);
        // Surface sessions support runtime direct↔packed A/B switching, so the
        // device must negotiate the loaded scene's packed sidecar requirement
        // even when the presenter is initially created on the Direct path.
        let scene = renderer
            .scene()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let (sh_width, sh_height) = sh_sidecar_atlas_dimensions(scene.len());
        required_texture_dimension = required_texture_dimension.max(sh_width).max(sh_height);
        if required_texture_dimension > adapter_limits.max_texture_dimension_2d {
            return Err(SurfacePresenterError::DeviceCreation(format!(
                "required texture dimension {required_texture_dimension} exceeds adapter limit {}",
                adapter_limits.max_texture_dimension_2d
            )));
        }
        required_limits.max_texture_dimension_2d = required_limits
            .max_texture_dimension_2d
            .max(required_texture_dimension);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-surface-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| {
                SurfacePresenterError::DeviceCreation(format!(
                    "{err}; adapter={adapter_info:?}; limits={adapter_limits:?}"
                ))
            })?;

        let caps = surface.get_capabilities(&adapter);
        let Some(format) = caps.formats.first().copied() else {
            return Err(SurfacePresenterError::NoSurfaceFormat);
        };
        let present_mode = select_present_mode(&caps);
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d.max(1);
        let (surface_width, surface_height) =
            fit_surface_size(width, height, max_texture_dimension_2d);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: surface_width,
            height: surface_height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &surface_config);
        if let Some(err) = error_scope.pop().await {
            return Err(SurfacePresenterError::SurfaceConfigure(err.to_string()));
        }

        let direct_bind_group_layout = create_direct_bind_group_layout(&device);
        let direct_pipeline = create_direct_pipeline(&device, &direct_bind_group_layout, format);
        let packed_bind_group_layout = packed_gpu::create_packed_bind_group_layout(&device);
        let packed_pipeline =
            packed_gpu::create_packed_pipeline(&device, &packed_bind_group_layout, format);
        let scene = renderer
            .scene()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let geometry_path = renderer.geometry_path();

        let (direct_scene, packed_scene) = match geometry_path {
            GeometryPath::SortedIndexDirect => {
                let world_covariance_terms = renderer
                    .world_covariance_terms
                    .as_deref()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                let alpha_values = renderer
                    .alpha_values
                    .as_deref()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                let direct_scene = DirectSceneResources::new(
                    &device,
                    &direct_bind_group_layout,
                    scene,
                    world_covariance_terms,
                    alpha_values,
                )?;
                (Some(direct_scene), None)
            }
            GeometryPath::PackedAtlas => {
                let packed_scene = packed_gpu::PackedAtlasResources::from_scene(
                    &device,
                    &queue,
                    &packed_bind_group_layout,
                    scene,
                )?;
                (None, Some(packed_scene))
            }
        };

        Ok(Self {
            surface,
            device,
            queue,
            direct_pipeline,
            direct_bind_group_layout,
            packed_pipeline,
            packed_bind_group_layout,
            surface_config,
            max_texture_dimension_2d,
            instance_count: 0,
            geometry_path,
            direct_scene,
            packed_scene,
            packed_color_refresh_position: None,
            packed_color_refresh_cursor: None,
            packed_color_refresh_target: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let (width, height) = fit_surface_size(width, height, self.max_texture_dimension_2d);
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub const fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        let latency = latency.clamp(1, 4);
        if self.surface_config.desired_maximum_frame_latency == latency {
            return;
        }

        self.surface_config.desired_maximum_frame_latency = latency;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub const fn geometry_path(&self) -> GeometryPath {
        self.geometry_path
    }

    /// Switches the Surface geometry path, clearing and rebuilding the GPU
    /// scene resources for the new path from the renderer's loaded scene.
    ///
    /// This is an experimental A/B benchmark knob: callers must keep
    /// `renderer`'s loaded scene in sync with the presenter that was created
    /// from it.
    pub fn set_geometry_path(
        &mut self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<(), SurfacePresenterError> {
        if self.geometry_path == path {
            return Ok(());
        }

        let scene = renderer
            .scene()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;

        self.direct_scene = None;
        self.packed_scene = None;
        self.packed_color_refresh_position = None;
        self.packed_color_refresh_cursor = None;
        self.packed_color_refresh_target = None;
        self.geometry_path = path;

        match path {
            GeometryPath::SortedIndexDirect => {
                let world_covariance_terms = renderer
                    .world_covariance_terms
                    .as_deref()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                let alpha_values = renderer
                    .alpha_values
                    .as_deref()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                self.direct_scene = Some(DirectSceneResources::new(
                    &self.device,
                    &self.direct_bind_group_layout,
                    scene,
                    world_covariance_terms,
                    alpha_values,
                )?);
            }
            GeometryPath::PackedAtlas => {
                self.packed_scene = Some(packed_gpu::PackedAtlasResources::from_scene(
                    &self.device,
                    &self.queue,
                    &self.packed_bind_group_layout,
                    scene,
                )?);
            }
        }

        Ok(())
    }

    pub fn render_sorted_indices(
        &mut self,
        scene: &SceneBuffers,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        match self.geometry_path {
            GeometryPath::SortedIndexDirect => {
                self.render_direct_frame(sorted_indices, camera, refresh_indices)
            }
            GeometryPath::PackedAtlas => {
                self.render_packed_frame(scene, sorted_indices, camera, refresh_indices)
            }
        }
    }

    fn render_direct_frame(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        let instance_count = self
            .direct_scene
            .as_ref()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                self.surface_config.width,
                self.surface_config.height,
                refresh_indices,
            )?;
        self.instance_count = instance_count;

        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(());
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-direct-encoder"),
            });
        {
            let direct_scene = self
                .direct_scene
                .as_ref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("gsplat-surface-direct-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.direct_pipeline);
            rpass.set_bind_group(0, &direct_scene.bind_group, &[]);
            if instance_count > 0 {
                rpass.draw(0..6, 0..instance_count);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn render_packed_frame(
        &mut self,
        scene: &SceneBuffers,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        let mut force_refresh = false;
        if self.packed_scene.is_none() {
            self.packed_scene = Some(packed_gpu::PackedAtlasResources::from_scene(
                &self.device,
                &self.queue,
                &self.packed_bind_group_layout,
                scene,
            )?);
            force_refresh = true;
            self.packed_color_refresh_position = None;
            self.packed_color_refresh_cursor = None;
            self.packed_color_refresh_target = None;
        }

        let position_key = packed_color_refresh_position_key(camera);
        {
            let packed_scene = self
                .packed_scene
                .as_ref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let needs_refresh = force_refresh
                || packed_color_refresh_needed(
                    self.packed_color_refresh_position,
                    camera,
                    packed_scene.bounds_min,
                    packed_scene.bounds_extent,
                );
            if force_refresh {
                let queue = self.queue.clone();
                let packed_scene = self
                    .packed_scene
                    .as_mut()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                refresh_packed_hot_colors(&queue, packed_scene, scene, camera);
                self.packed_color_refresh_position = Some(position_key);
                self.packed_color_refresh_cursor = None;
                self.packed_color_refresh_target = None;
            } else if !refresh_indices
                && (self.packed_color_refresh_cursor.is_some() || needs_refresh)
            {
                // Defer banded SH refresh off synchronous sort frames so p95
                // does not stack a full CPU sort with SH eval/upload.
                if self.packed_color_refresh_cursor.is_none() {
                    self.packed_color_refresh_cursor = Some(0);
                    self.packed_color_refresh_target = Some(position_key);
                }
                let start = self.packed_color_refresh_cursor.unwrap_or(0);
                let end = (start + packed_color_refresh_band_size(scene.len())).min(scene.len());
                let queue = self.queue.clone();
                let packed_scene = self
                    .packed_scene
                    .as_mut()
                    .ok_or(SurfacePresenterError::SceneNotLoaded)?;
                refresh_packed_hot_colors_range(&queue, packed_scene, scene, camera, start, end);
                if end >= scene.len() {
                    self.packed_color_refresh_position = self.packed_color_refresh_target;
                    self.packed_color_refresh_cursor = None;
                    self.packed_color_refresh_target = None;
                } else {
                    self.packed_color_refresh_cursor = Some(end);
                }
            }
        }

        let instance_count = self
            .packed_scene
            .as_ref()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                self.surface_config.width,
                self.surface_config.height,
                refresh_indices,
            )?;
        self.instance_count = instance_count;

        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(());
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-packed-encoder"),
            });
        {
            let packed_scene = self
                .packed_scene
                .as_ref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("gsplat-surface-packed-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.packed_pipeline);
            rpass.set_bind_group(0, &packed_scene.bind_group, &[]);
            if instance_count > 0 {
                rpass.draw(0..packed_gpu::PACKED_QUAD_VERTEX_COUNT, 0..instance_count);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn acquire_surface_texture(
        &mut self,
    ) -> Result<Option<wgpu::SurfaceTexture>, SurfacePresenterError> {
        match self.surface.get_current_texture() {
            Ok(frame) => Ok(Some(frame)),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    Ok(frame) => Ok(Some(frame)),
                    Err(wgpu::SurfaceError::Timeout) => Ok(None),
                    Err(err) => Err(surface_error_to_presenter(err)),
                }
            }
            Err(wgpu::SurfaceError::Timeout) => Ok(None),
            Err(err) => Err(surface_error_to_presenter(err)),
        }
    }

    pub const fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

fn make_surface_source_elems(
    scene: &SceneBuffers,
    world_covariance_terms: &[CameraCovarianceTerms],
    alpha_values: &[f32],
) -> Vec<GpuSurfaceSourceElem> {
    if scene.positions.is_empty() {
        return vec![GpuSurfaceSourceElem::zeroed()];
    }

    (0..scene.positions.len())
        .map(|i| {
            let position = scene.positions[i];
            let color_dc = scene.color_dc.get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
            let cov = world_covariance_terms
                .get(i)
                .copied()
                .unwrap_or(CameraCovarianceTerms {
                    xx: 0.0,
                    xy: 0.0,
                    xz: 0.0,
                    yy: 0.0,
                    yz: 0.0,
                    zz: 0.0,
                });
            let alpha = alpha_values.get(i).copied().unwrap_or(0.0);
            GpuSurfaceSourceElem {
                position: [position.x, position.y, position.z, 0.0],
                covariance0: [cov.xx, cov.xy, cov.xz, cov.yy],
                covariance1: [cov.yz, cov.zz, alpha, 0.0],
                color_dc: [color_dc[0], color_dc[1], color_dc[2], 0.0],
            }
        })
        .collect()
}

pub(crate) fn make_surface_render_params(
    camera: &Camera,
    width: u32,
    height: u32,
    len: u32,
    sh_degree: u32,
) -> GpuSurfaceRenderParams {
    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    GpuSurfaceRenderParams {
        camera_pos: [
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
            0.0,
        ],
        view_rot_row0: [view_rot[0][0], view_rot[0][1], view_rot[0][2], 0.0],
        view_rot_row1: [view_rot[1][0], view_rot[1][1], view_rot[1][2], 0.0],
        view_rot_row2: [view_rot[2][0], view_rot[2][1], view_rot[2][2], 0.0],
        vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
        near_plane: camera.intrinsics.near_plane,
        far_plane: camera.intrinsics.far_plane,
        aspect: (width as f32 / height.max(1) as f32).max(1e-6),
        width,
        height,
        sh_degree,
        len,
    }
}

fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        return wgpu::PresentMode::Mailbox;
    }
    if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        return wgpu::PresentMode::Fifo;
    }

    caps.present_modes
        .first()
        .copied()
        .unwrap_or(wgpu::PresentMode::Fifo)
}

fn surface_error_to_presenter(err: wgpu::SurfaceError) -> SurfacePresenterError {
    match err {
        wgpu::SurfaceError::OutOfMemory => SurfacePresenterError::SurfaceOutOfMemory,
        other => SurfacePresenterError::SurfaceAcquire(format!("{other:?}")),
    }
}

fn fit_surface_size(width: u32, height: u32, max_dimension: u32) -> (u32, u32) {
    let max_input_dimension = width.max(height).max(1);
    if max_input_dimension <= max_dimension {
        return (width, height);
    }

    let scale = max_dimension as f32 / max_input_dimension as f32;
    let scaled_width = ((width as f32) * scale).round() as u32;
    let scaled_height = ((height as f32) * scale).round() as u32;
    (scaled_width.max(1), scaled_height.max(1))
}

#[cfg(target_os = "android")]
fn create_surface_instance() -> wgpu::Instance {
    wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    })
}

#[cfg(not(target_os = "android"))]
fn create_surface_instance() -> wgpu::Instance {
    wgpu::Instance::default()
}

fn is_visible(depth_z: f32, camera: &Camera) -> bool {
    depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane
}

fn depth_to_key(depth_z: f32) -> u32 {
    // Positive finite depth values preserve a monotonic relationship when using IEEE-754 bits.
    depth_z.max(0.0).to_bits()
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuInstance {
    // xy = center in NDC, zw = major axis in NDC
    pub center_and_axis_u: [f32; 4],
    // xy = minor axis in NDC, zw = reserved
    pub axis_v_and_pad: [f32; 4],
    // Premultiplied RGB + alpha
    pub color_rgba: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSurfaceSourceElem {
    position: [f32; 4],
    covariance0: [f32; 4],
    covariance1: [f32; 4],
    color_dc: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuSurfaceRenderParams {
    camera_pos: [f32; 4],
    view_rot_row0: [f32; 4],
    view_rot_row1: [f32; 4],
    view_rot_row2: [f32; 4],
    vertical_fov_radians: f32,
    near_plane: f32,
    far_plane: f32,
    aspect: f32,
    width: u32,
    height: u32,
    sh_degree: u32,
    len: u32,
}

#[cfg(test)]
fn build_instances(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
) -> Vec<GpuInstance> {
    let mut out = Vec::new();
    build_instances_into(
        scene,
        world_covariances,
        alpha_values,
        indices,
        camera,
        config,
        &mut out,
    );
    out
}

fn build_instances_into(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
    out: &mut Vec<GpuInstance>,
) {
    if world_covariances.len() != scene.len() || alpha_values.len() != scene.len() {
        out.clear();
        return;
    }

    let Some(params) = InstanceBuildParams::new(camera, config) else {
        out.clear();
        return;
    };

    if out.len() < indices.len() {
        out.resize(indices.len(), GpuInstance::zeroed());
    } else {
        out.truncate(indices.len());
    }

    let sh_layout = ShColorLayout::new(scene);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let had_invalid = AtomicBool::new(false);
        out.par_iter_mut()
            .zip(indices.par_iter())
            .for_each(|(slot, &idx)| {
                let i = idx as usize;
                let instance = if i < scene.len() {
                    // SAFETY: the explicit bounds check above covers all scene-parallel arrays
                    // because the caller validated equal lengths before entering this loop.
                    unsafe {
                        build_instance_unchecked(
                            scene,
                            world_covariances,
                            alpha_values,
                            i,
                            camera,
                            &params,
                            sh_layout,
                        )
                    }
                } else {
                    None
                };
                if let Some(instance) = instance {
                    *slot = instance;
                } else {
                    *slot = invalid_gpu_instance();
                    had_invalid.store(true, Ordering::Relaxed);
                }
            });

        if had_invalid.load(Ordering::Relaxed) {
            out.retain(|instance| instance.color_rgba[3] >= 0.0);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut write_index = 0_usize;
        for &idx in indices {
            let i = idx as usize;
            let instance = if i < scene.len() {
                // SAFETY: the explicit bounds check above covers all scene-parallel arrays
                // because the caller validated equal lengths before entering this loop.
                unsafe {
                    build_instance_unchecked(
                        scene,
                        world_covariances,
                        alpha_values,
                        i,
                        camera,
                        &params,
                        sh_layout,
                    )
                }
            } else {
                None
            };
            if let Some(instance) = instance {
                out[write_index] = instance;
                write_index += 1;
            }
        }
        out.truncate(write_index);
    }
}

unsafe fn build_instance_unchecked(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    i: usize,
    camera: &Camera,
    params: &InstanceBuildParams,
    sh_layout: ShColorLayout<'_>,
) -> Option<GpuInstance> {
    let pos_world = unsafe { *scene.positions.get_unchecked(i) };
    let p_cam = world_to_camera_with_view_rot(pos_world, camera.pose.position, params.view_rot);
    // Preprocess already culled by z range, but keep this safe for runtime camera changes.
    if !is_visible(p_cam.z, camera) {
        return None;
    }

    // Project center to NDC for instance placement.
    let inv_z = 1.0 / p_cam.z.max(1e-6);
    let x_ndc = (p_cam.x * params.f) * inv_z / params.aspect;
    let y_ndc = (p_cam.y * params.f) * inv_z;

    let cov2_ndc = project_world_covariance_to_ndc(
        p_cam,
        unsafe { *world_covariances.get_unchecked(i) },
        params,
    )?;
    let (axis_u, axis_v) = ellipse_axes_from_covariance(cov2_ndc)?;
    let extent_x = axis_u[0].abs() + axis_v[0].abs();
    let extent_y = axis_u[1].abs() + axis_v[1].abs();
    if x_ndc + extent_x < -1.0
        || x_ndc - extent_x > 1.0
        || y_ndc + extent_y < -1.0
        || y_ndc - extent_y > 1.0
    {
        return None;
    }

    let alpha = unsafe { *alpha_values.get_unchecked(i) };
    let dir_world = normalize3(Vec3f::new(
        pos_world.x - camera.pose.position.x,
        pos_world.y - camera.pose.position.y,
        pos_world.z - camera.pose.position.z,
    ));
    let rgb = unsafe { sh_color_unchecked(scene, i, dir_world, sh_layout) };
    let rgb = [
        rgb[0].clamp(0.0, 1.0),
        rgb[1].clamp(0.0, 1.0),
        rgb[2].clamp(0.0, 1.0),
    ];

    Some(GpuInstance {
        center_and_axis_u: [x_ndc, y_ndc, axis_u[0], axis_u[1]],
        axis_v_and_pad: [axis_v[0], axis_v[1], 0.0, 0.0],
        color_rgba: [
            (rgb[0] * alpha).clamp(0.0, 1.0),
            (rgb[1] * alpha).clamp(0.0, 1.0),
            (rgb[2] * alpha).clamp(0.0, 1.0),
            alpha,
        ],
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn invalid_gpu_instance() -> GpuInstance {
    GpuInstance {
        color_rgba: [0.0, 0.0, 0.0, -1.0],
        ..GpuInstance::zeroed()
    }
}

#[derive(Clone, Copy)]
struct InstanceBuildParams {
    aspect: f32,
    f: f32,
    fx: f32,
    fy: f32,
    lim_x: f32,
    lim_y: f32,
    blur_cov_x: f32,
    blur_cov_y: f32,
    view_rot: [[f32; 3]; 3],
}

impl InstanceBuildParams {
    fn new(camera: &Camera, config: RendererConfig) -> Option<Self> {
        if config.width == 0 || config.height == 0 {
            return None;
        }
        let tan_half_fovy = (camera.intrinsics.vertical_fov_radians * 0.5).tan();
        if tan_half_fovy <= 0.0 || !tan_half_fovy.is_finite() {
            return None;
        }

        let aspect = config.width as f32 / config.height as f32;
        let f = 1.0 / tan_half_fovy;
        let fx = f / aspect;
        let fy = f;
        let tan_half_fovx = tan_half_fovy * aspect;
        let blur_pixels = 0.3_f32;
        let px_ndc_x = 2.0 / config.width as f32;
        let px_ndc_y = 2.0 / config.height as f32;
        let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);

        Some(Self {
            aspect,
            f,
            fx,
            fy,
            lim_x: 1.3 * tan_half_fovx,
            lim_y: 1.3 * tan_half_fovy,
            blur_cov_x: (blur_pixels * px_ndc_x).powi(2),
            blur_cov_y: (blur_pixels * px_ndc_y).powi(2),
            view_rot: quat_to_mat3(camera_inv_q),
        })
    }
}

fn precompute_alpha_values(scene: &SceneBuffers) -> Vec<f32> {
    scene
        .opacity
        .iter()
        .map(|&opacity| sigmoid(opacity).clamp(0.0, 1.0))
        .collect()
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn preprocess_visible_into(
    scene: &SceneBuffers,
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    depth_keys.clear();
    indices.clear();
    if depth_keys.capacity() < scene.len() {
        depth_keys.reserve(scene.len() - depth_keys.capacity());
    }
    if indices.capacity() < scene.len() {
        indices.reserve(scene.len() - indices.capacity());
    }

    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;

    for (idx, position) in scene.positions.iter().enumerate() {
        let depth_z = world_to_camera_depth_with_view_row(*position, camera_position, depth_row);
        if is_visible(depth_z, camera) {
            indices.push(idx as u32);
            depth_keys.push(depth_to_key(depth_z));
        }
    }

    Ok(())
}

fn world_to_camera_depth_with_view_row(
    pos_world: Vec3f,
    camera_position: Vec3f,
    depth_row: [f32; 3],
) -> f32 {
    depth_row[0] * (pos_world.x - camera_position.x)
        + depth_row[1] * (pos_world.y - camera_position.y)
        + depth_row[2] * (pos_world.z - camera_position.z)
}

fn world_to_camera_with_view_rot(
    pos_world: Vec3f,
    camera_position: Vec3f,
    view_rot: [[f32; 3]; 3],
) -> Vec3f {
    let p = Vec3f::new(
        pos_world.x - camera_position.x,
        pos_world.y - camera_position.y,
        pos_world.z - camera_position.z,
    );

    Vec3f::new(
        view_rot[0][0] * p.x + view_rot[0][1] * p.y + view_rot[0][2] * p.z,
        view_rot[1][0] * p.x + view_rot[1][1] * p.y + view_rot[1][2] * p.z,
        view_rot[2][0] * p.x + view_rot[2][1] * p.y + view_rot[2][2] * p.z,
    )
}

fn quat_inverse(q: [f32; 4]) -> [f32; 4] {
    let q = quat_normalize(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [-x, -y, -z, w]
}

fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let norm2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if norm2 <= 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 / norm2.sqrt();
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

fn quat_to_mat3(q: [f32; 4]) -> [[f32; 3]; 3] {
    let q = quat_normalize(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;

    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
        [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
        [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
    ]
}

fn mat3_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

fn mat3_transpose(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

#[cfg(test)]
fn project_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: [[f32; 3]; 3],
    camera: &Camera,
    config: RendererConfig,
) -> Option<[[f32; 2]; 2]> {
    let params = InstanceBuildParams::new(camera, config)?;
    project_camera_covariance_to_ndc(p_cam, CameraCovarianceTerms::from_matrix(cov_cam), &params)
}

fn project_world_covariance_to_ndc(
    p_cam: Vec3f,
    world_cov: [[f32; 3]; 3],
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let world_cov = CameraCovarianceTerms::from_matrix(world_cov);
    project_world_covariance_terms_to_ndc(p_cam, world_cov, params)
}

fn project_world_covariance_terms_to_ndc(
    p_cam: Vec3f,
    world_cov: CameraCovarianceTerms,
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let cov_cam = transform_covariance_terms_to_camera(world_cov, params.view_rot);
    project_camera_covariance_to_ndc(p_cam, cov_cam, params)
}

fn project_camera_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: CameraCovarianceTerms,
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let z = p_cam.z;
    if z <= 1e-6 || !z.is_finite() {
        return None;
    }

    // Match common 3DGS covariance projection behavior: clamp view-space x/z and y/z
    // before Jacobian evaluation to avoid extreme derivatives at frustum edges.
    let x_clamped = (p_cam.x / z).clamp(-params.lim_x, params.lim_x) * z;
    let y_clamped = (p_cam.y / z).clamp(-params.lim_y, params.lim_y) * z;

    let inv_z = 1.0 / z;
    let inv_z2 = inv_z * inv_z;
    let j00 = params.fx * inv_z;
    let j02 = -params.fx * x_clamped * inv_z2;
    let j11 = params.fy * inv_z;
    let j12 = -params.fy * y_clamped * inv_z2;

    let cov01 = j00 * j11 * cov_cam.xy
        + j00 * j12 * cov_cam.xz
        + j02 * j11 * cov_cam.yz
        + j02 * j12 * cov_cam.zz;
    let mut cov2 = [
        [
            j00 * j00 * cov_cam.xx + 2.0 * j00 * j02 * cov_cam.xz + j02 * j02 * cov_cam.zz,
            cov01,
        ],
        [
            cov01,
            j11 * j11 * cov_cam.yy + 2.0 * j11 * j12 * cov_cam.yz + j12 * j12 * cov_cam.zz,
        ],
    ];

    // Low-pass filter in NDC space to keep splats from collapsing to subpixel noise.
    cov2[0][0] += params.blur_cov_x;
    cov2[1][1] += params.blur_cov_y;

    if !cov2[0][0].is_finite()
        || !cov2[0][1].is_finite()
        || !cov2[1][0].is_finite()
        || !cov2[1][1].is_finite()
    {
        return None;
    }

    Some(cov2)
}

#[derive(Clone, Copy)]
struct CameraCovarianceTerms {
    xx: f32,
    xy: f32,
    xz: f32,
    yy: f32,
    yz: f32,
    zz: f32,
}

impl CameraCovarianceTerms {
    fn from_matrix(cov: [[f32; 3]; 3]) -> Self {
        Self {
            xx: cov[0][0],
            xy: cov[0][1],
            xz: cov[0][2],
            yy: cov[1][1],
            yz: cov[1][2],
            zz: cov[2][2],
        }
    }
}

fn transform_covariance_terms_to_camera(
    cov: CameraCovarianceTerms,
    view_rot: [[f32; 3]; 3],
) -> CameraCovarianceTerms {
    let c00 = cov.xx;
    let c01 = cov.xy;
    let c02 = cov.xz;
    let c11 = cov.yy;
    let c12 = cov.yz;
    let c22 = cov.zz;
    let r0 = view_rot[0];
    let r1 = view_rot[1];
    let r2 = view_rot[2];

    CameraCovarianceTerms {
        xx: covariance_quadratic(c00, c01, c02, c11, c12, c22, r0),
        xy: covariance_bilinear(c00, c01, c02, c11, c12, c22, r0, r1),
        xz: covariance_bilinear(c00, c01, c02, c11, c12, c22, r0, r2),
        yy: covariance_quadratic(c00, c01, c02, c11, c12, c22, r1),
        yz: covariance_bilinear(c00, c01, c02, c11, c12, c22, r1, r2),
        zz: covariance_quadratic(c00, c01, c02, c11, c12, c22, r2),
    }
}

fn covariance_quadratic(
    c00: f32,
    c01: f32,
    c02: f32,
    c11: f32,
    c12: f32,
    c22: f32,
    r: [f32; 3],
) -> f32 {
    r[0] * r[0] * c00
        + 2.0 * r[0] * r[1] * c01
        + 2.0 * r[0] * r[2] * c02
        + r[1] * r[1] * c11
        + 2.0 * r[1] * r[2] * c12
        + r[2] * r[2] * c22
}

#[allow(clippy::too_many_arguments)]
fn covariance_bilinear(
    c00: f32,
    c01: f32,
    c02: f32,
    c11: f32,
    c12: f32,
    c22: f32,
    a: [f32; 3],
    b: [f32; 3],
) -> f32 {
    let bx = c00 * b[0] + c01 * b[1] + c02 * b[2];
    let by = c01 * b[0] + c11 * b[1] + c12 * b[2];
    let bz = c02 * b[0] + c12 * b[1] + c22 * b[2];
    a[0] * bx + a[1] * by + a[2] * bz
}

fn ellipse_axes_from_covariance(cov2: [[f32; 2]; 2]) -> Option<([f32; 2], [f32; 2])> {
    let a = cov2[0][0];
    let b = cov2[0][1];
    let c = cov2[1][1];
    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return None;
    }

    let apco2 = (a + c) * 0.5;
    let amco2 = (a - c) * 0.5;
    let term = (amco2 * amco2 + b * b).sqrt();
    let major = (apco2 + term).max(1e-10);
    let minor = (apco2 - term).max(1e-10);

    let axis_u_dir = if b.abs() > 1e-8 {
        normalize2([b, major - a])
    } else if a >= c {
        [1.0, 0.0]
    } else {
        [0.0, 1.0]
    };
    let axis_v_dir = [-axis_u_dir[1], axis_u_dir[0]];

    // 3-sigma support region for rasterization.
    let radius_k = 3.0_f32;
    let mut major_radius = (major.sqrt() * radius_k).clamp(1e-4, 2.0);
    let minor_radius = (minor.sqrt() * radius_k).clamp(1e-4, 2.0);

    // Guard against extreme anisotropy from unstable covariance outliers. Those splats tend to
    // show up as needle-like streaks while contributing little stable structure.
    const MAX_ANISOTROPY: f32 = 64.0;
    if major_radius > minor_radius * MAX_ANISOTROPY {
        major_radius = minor_radius * MAX_ANISOTROPY;
    }
    let axis_u = [axis_u_dir[0] * major_radius, axis_u_dir[1] * major_radius];
    let axis_v = [axis_v_dir[0] * minor_radius, axis_v_dir[1] * minor_radius];
    if !axis_u[0].is_finite()
        || !axis_u[1].is_finite()
        || !axis_v[0].is_finite()
        || !axis_v[1].is_finite()
    {
        return None;
    }

    Some((axis_u, axis_v))
}

fn normalize3(v: Vec3f) -> [f32; 3] {
    let len2 = v.x * v.x + v.y * v.y + v.z * v.z;
    if len2 <= 0.0 {
        return [0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v.x * inv, v.y * inv, v.z * inv]
}

fn normalize2(v: [f32; 2]) -> [f32; 2] {
    let len2 = v[0] * v[0] + v[1] * v[1];
    if len2 <= 0.0 {
        return [1.0, 0.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v[0] * inv, v[1] * inv]
}

fn precompute_world_covariances(scene: &SceneBuffers) -> Vec<[[f32; 3]; 3]> {
    let mut out = Vec::with_capacity(scene.len());
    for i in 0..scene.len() {
        let scale = scene.scale_xyz[i];
        let sx = scale[0].exp().max(1e-6);
        let sy = scale[1].exp().max(1e-6);
        let sz = scale[2].exp().max(1e-6);
        let object_cov = [
            [sx * sx, 0.0, 0.0],
            [0.0, sy * sy, 0.0],
            [0.0, 0.0, sz * sz],
        ];
        let rot_gaussian = quat_to_mat3(quat_normalize(scene.rotation_xyzw[i]));
        let world_cov = mat3_mul(
            mat3_mul(rot_gaussian, object_cov),
            mat3_transpose(rot_gaussian),
        );
        out.push(world_cov);
    }
    out
}

#[derive(Clone, Copy)]
struct ShColorLayout<'a> {
    rest: Option<&'a [f32]>,
    degree: u8,
    per_channel: usize,
    stride: usize,
}

impl<'a> ShColorLayout<'a> {
    fn new(scene: &'a SceneBuffers) -> Self {
        let rest = scene.sh_rest.as_deref();
        let coeff_total = if rest.is_some() {
            (scene.sh_degree as usize + 1).pow(2)
        } else {
            1
        };
        let per_channel = coeff_total.saturating_sub(1);
        Self {
            rest,
            degree: if rest.is_some() { scene.sh_degree } else { 0 },
            per_channel,
            stride: per_channel * 3,
        }
    }
}

unsafe fn sh_color_unchecked(
    scene: &SceneBuffers,
    index: usize,
    dir: [f32; 3],
    layout: ShColorLayout<'_>,
) -> [f32; 3] {
    // The PLY stores SH coefficients as `f_dc_*` + `f_rest_*`. Evaluate as in 3DGS:
    // `rgb = clamp_min(eval_sh(deg, sh, dir) + 0.5, 0.0)`.
    // Reference: graphdeco-inria/gaussian-splatting `utils/sh_utils.py`.
    const C0: f32 = 0.282_094_8_f32;
    let dc = unsafe { *scene.color_dc.get_unchecked(index) };
    let mut rgb = [C0 * dc[0], C0 * dc[1], C0 * dc[2]];

    if let Some(rest) = layout.rest {
        let base = index * layout.stride;
        if layout.degree == 3
            && layout.per_channel == 15
            && let Some(end) = base.checked_add(45)
            && end <= rest.len()
        {
            let sh_rgb = sh_color_rest_deg3(dir, &rest[base..end]);
            return [
                (rgb[0] + sh_rgb[0] + 0.5).max(0.0),
                (rgb[1] + sh_rgb[1] + 0.5).max(0.0),
                (rgb[2] + sh_rgb[2] + 0.5).max(0.0),
            ];
        }

        let (basis, basis_len) = sh_basis(layout.degree, dir);
        for (channel, value) in rgb.iter_mut().enumerate() {
            let channel_base = base + channel * layout.per_channel;
            if channel_base >= rest.len() {
                continue;
            }
            let available = rest.len() - channel_base;
            let term_count = basis_len.min(layout.per_channel).min(available);
            *value += dot_sh_terms(&basis, &rest[channel_base..], term_count);
        }
    }

    [
        (rgb[0] + 0.5).max(0.0),
        (rgb[1] + 0.5).max(0.0),
        (rgb[2] + 0.5).max(0.0),
    ]
}

fn normalize_dir(dx: f32, dy: f32, dz: f32) -> [f32; 3] {
    let len2 = dx * dx + dy * dy + dz * dz;
    if len2 <= 1e-20 {
        return [0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len2.sqrt();
    [dx * inv, dy * inv, dz * inv]
}

/// Color-refresh: evaluate float SH into the packed hot RGB10 stream.
fn refresh_packed_hot_colors(
    queue: &wgpu::Queue,
    packed: &mut packed_gpu::PackedAtlasResources,
    scene: &SceneBuffers,
    camera: &Camera,
) {
    refresh_packed_hot_colors_range(queue, packed, scene, camera, 0, scene.len());
}

/// Refresh a half-open splat range so orbit p95 is not dominated by one full-scene
/// SH evaluation + upload spike.
fn refresh_packed_hot_colors_range(
    queue: &wgpu::Queue,
    packed: &mut packed_gpu::PackedAtlasResources,
    scene: &SceneBuffers,
    camera: &Camera,
    start: usize,
    end: usize,
) {
    if start >= end || end > scene.len() {
        return;
    }
    let layout = ShColorLayout::new(scene);
    let cam = [
        camera.pose.position.x,
        camera.pose.position.y,
        camera.pose.position.z,
    ];
    let mut colors = vec![0_u32; scene.len()];

    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        colors[start..end]
            .par_iter_mut()
            .enumerate()
            .for_each(|(offset, slot)| {
                let index = start + offset;
                let position = scene.positions[index];
                let dir = normalize_dir(
                    position.x - cam[0],
                    position.y - cam[1],
                    position.z - cam[2],
                );
                let rgb = unsafe { sh_color_unchecked(scene, index, dir, layout) };
                let rgb = [
                    rgb[0].clamp(0.0, 1.0),
                    rgb[1].clamp(0.0, 1.0),
                    rgb[2].clamp(0.0, 1.0),
                ];
                *slot = pack_color_rgb10(rgb);
            });
    }

    #[cfg(target_arch = "wasm32")]
    {
        for (index, slot) in colors.iter_mut().enumerate().take(end).skip(start) {
            let position = scene.positions[index];
            let dir = normalize_dir(
                position.x - cam[0],
                position.y - cam[1],
                position.z - cam[2],
            );
            let rgb = unsafe { sh_color_unchecked(scene, index, dir, layout) };
            let rgb = [
                rgb[0].clamp(0.0, 1.0),
                rgb[1].clamp(0.0, 1.0),
                rgb[2].clamp(0.0, 1.0),
            ];
            *slot = pack_color_rgb10(rgb);
        }
    }

    packed.write_hot_colors_range(queue, &colors, start, end);
}

/// Upper bound on splats refreshed per frame during steady-state motion.
/// Keeps synchronous orbit p95 from absorbing a full-scene SH spike.
fn packed_color_refresh_band_size(splat_count: usize) -> usize {
    const MIN_BAND: usize = 8_192;
    const MAX_BAND: usize = 24_576;
    let eighth = splat_count.div_ceil(8);
    eighth.clamp(MIN_BAND, MAX_BAND).min(splat_count.max(1))
}

fn packed_color_refresh_position_key(camera: &Camera) -> [f32; 3] {
    [
        camera.pose.position.x,
        camera.pose.position.y,
        camera.pose.position.z,
    ]
}

/// Refresh when accumulated camera translation can change any scene-point view
/// direction by roughly ten degrees. Camera rotation alone never changes SH
/// color because SH is evaluated from source position to camera position.
const PACKED_COLOR_REFRESH_COS_THRESHOLD: f32 = 0.984_807_8; // cos(10°)

fn packed_color_refresh_needed(
    previous: Option<[f32; 3]>,
    camera: &Camera,
    bounds_min: [f32; 3],
    bounds_extent: [f32; 3],
) -> bool {
    let Some(prev_pos) = previous else {
        return true;
    };
    let pos = [
        camera.pose.position.x,
        camera.pose.position.y,
        camera.pose.position.z,
    ];
    let dp = [
        pos[0] - prev_pos[0],
        pos[1] - prev_pos[1],
        pos[2] - prev_pos[2],
    ];
    let displacement_sq = dp[0] * dp[0] + dp[1] * dp[1] + dp[2] * dp[2];
    if displacement_sq <= f32::EPSILON {
        return false;
    }

    let center = [
        bounds_min[0] + bounds_extent[0] * 0.5,
        bounds_min[1] + bounds_extent[1] * 0.5,
        bounds_min[2] + bounds_extent[2] * 0.5,
    ];
    let radius = 0.5
        * (bounds_extent[0] * bounds_extent[0]
            + bounds_extent[1] * bounds_extent[1]
            + bounds_extent[2] * bounds_extent[2])
            .sqrt();
    let center_delta = [
        prev_pos[0] - center[0],
        prev_pos[1] - center[1],
        prev_pos[2] - center[2],
    ];
    let center_distance_sq = center_delta[0] * center_delta[0]
        + center_delta[1] * center_delta[1]
        + center_delta[2] * center_delta[2];
    let clearance = center_distance_sq.sqrt() - radius;
    if clearance <= 1e-6 {
        return true;
    }
    let sin_threshold = (1.0
        - PACKED_COLOR_REFRESH_COS_THRESHOLD * PACKED_COLOR_REFRESH_COS_THRESHOLD)
        .max(0.0)
        .sqrt();
    let allowed_displacement = clearance * sin_threshold;
    displacement_sq > allowed_displacement * allowed_displacement
}

fn sh_color_rest_deg3(dir: [f32; 3], rest: &[f32]) -> [f32; 3] {
    debug_assert!(rest.len() >= 45);

    const C1: f32 = 0.488_602_52_f32;
    const C2: [f32; 5] = [
        1.092_548_5_f32,
        -1.092_548_5_f32,
        0.315_391_57_f32,
        -1.092_548_5_f32,
        0.546_274_24_f32,
    ];
    const C3: [f32; 7] = [
        -0.590_043_6_f32,
        2.890_611_4_f32,
        -0.457_045_8_f32,
        0.373_176_34_f32,
        -0.457_045_8_f32,
        1.445_305_7_f32,
        -0.590_043_6_f32,
    ];

    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    let b0 = -C1 * y;
    let b1 = C1 * z;
    let b2 = -C1 * x;
    let b3 = C2[0] * xy;
    let b4 = C2[1] * yz;
    let b5 = C2[2] * (2.0 * zz - xx - yy);
    let b6 = C2[3] * xz;
    let b7 = C2[4] * (xx - yy);
    let b8 = C3[0] * y * (3.0 * xx - yy);
    let b9 = C3[1] * xy * z;
    let b10 = C3[2] * y * (4.0 * zz - xx - yy);
    let b11 = C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
    let b12 = C3[4] * x * (4.0 * zz - xx - yy);
    let b13 = C3[5] * z * (xx - yy);
    let b14 = C3[6] * x * (xx - 3.0 * yy);

    [
        sh_dot15(
            rest, b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14,
        ),
        sh_dot15(
            &rest[15..],
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
        ),
        sh_dot15(
            &rest[30..],
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn sh_dot15(
    rest: &[f32],
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
    b7: f32,
    b8: f32,
    b9: f32,
    b10: f32,
    b11: f32,
    b12: f32,
    b13: f32,
    b14: f32,
) -> f32 {
    debug_assert!(rest.len() >= 15);
    b0 * rest[0]
        + b1 * rest[1]
        + b2 * rest[2]
        + b3 * rest[3]
        + b4 * rest[4]
        + b5 * rest[5]
        + b6 * rest[6]
        + b7 * rest[7]
        + b8 * rest[8]
        + b9 * rest[9]
        + b10 * rest[10]
        + b11 * rest[11]
        + b12 * rest[12]
        + b13 * rest[13]
        + b14 * rest[14]
}

fn sh_basis(deg: u8, dir: [f32; 3]) -> ([f32; 24], usize) {
    const C1: f32 = 0.488_602_52_f32;
    const C2: [f32; 5] = [
        1.092_548_5_f32,
        -1.092_548_5_f32,
        0.315_391_57_f32,
        -1.092_548_5_f32,
        0.546_274_24_f32,
    ];
    const C3: [f32; 7] = [
        -0.590_043_6_f32,
        2.890_611_4_f32,
        -0.457_045_8_f32,
        0.373_176_34_f32,
        -0.457_045_8_f32,
        1.445_305_7_f32,
        -0.590_043_6_f32,
    ];
    const C4: [f32; 9] = [
        2.503_342_9_f32,
        -1.770_130_8_f32,
        0.946_174_7_f32,
        -0.669_046_5_f32,
        0.105_785_55_f32,
        -0.669_046_5_f32,
        0.473_087_34_f32,
        -1.770_130_8_f32,
        0.625_835_7_f32,
    ];

    let mut basis = [0.0_f32; 24];
    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    if deg == 0 {
        return (basis, 0);
    }

    basis[0] = -C1 * y;
    basis[1] = C1 * z;
    basis[2] = -C1 * x;
    if deg == 1 {
        return (basis, 3);
    }

    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    basis[3] = C2[0] * xy;
    basis[4] = C2[1] * yz;
    basis[5] = C2[2] * (2.0 * zz - xx - yy);
    basis[6] = C2[3] * xz;
    basis[7] = C2[4] * (xx - yy);
    if deg == 2 {
        return (basis, 8);
    }

    basis[8] = C3[0] * y * (3.0 * xx - yy);
    basis[9] = C3[1] * xy * z;
    basis[10] = C3[2] * y * (4.0 * zz - xx - yy);
    basis[11] = C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
    basis[12] = C3[4] * x * (4.0 * zz - xx - yy);
    basis[13] = C3[5] * z * (xx - yy);
    basis[14] = C3[6] * x * (xx - 3.0 * yy);
    if deg == 3 {
        return (basis, 15);
    }

    // deg 4 support (rare for 3DGS, but safe to handle).
    basis[15] = C4[0] * xy * (xx - yy);
    basis[16] = C4[1] * yz * (3.0 * xx - yy);
    basis[17] = C4[2] * xy * (7.0 * zz - 1.0);
    basis[18] = C4[3] * yz * (7.0 * zz - 3.0);
    basis[19] = C4[4] * (zz * (35.0 * zz - 30.0) + 3.0);
    basis[20] = C4[5] * xz * (7.0 * zz - 3.0);
    basis[21] = C4[6] * (xx - yy) * (7.0 * zz - 1.0);
    basis[22] = C4[7] * xz * (xx - 3.0 * yy);
    basis[23] = C4[8] * (xx * (xx - 3.0 * yy) - yy * (3.0 * xx - yy));
    (basis, 24)
}

fn dot_sh_terms(basis: &[f32; 24], rest: &[f32], count: usize) -> f32 {
    debug_assert!(count <= basis.len());
    debug_assert!(count <= rest.len());

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: AArch64 guarantees Neon availability and the count has been bounds-checked.
        unsafe { dot_sh_terms_neon(basis, rest, count) }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut result = 0.0_f32;
        for i in 0..count {
            result += basis[i] * rest[i];
        }
        result
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn dot_sh_terms_neon(basis: &[f32; 24], rest: &[f32], count: usize) -> f32 {
    use std::arch::aarch64::*;

    let mut sum4 = vdupq_n_f32(0.0);
    let mut i = 0_usize;
    while i + 4 <= count {
        let b = unsafe { vld1q_f32(basis.as_ptr().add(i)) };
        let r = unsafe { vld1q_f32(rest.as_ptr().add(i)) };
        sum4 = vmlaq_f32(sum4, b, r);
        i += 4;
    }

    let mut lanes = [0.0_f32; 4];
    unsafe { vst1q_f32(lanes.as_mut_ptr(), sum4) };
    let mut result = lanes[0] + lanes[1] + lanes[2] + lanes[3];
    while i < count {
        result += unsafe { *basis.as_ptr().add(i) } * unsafe { *rest.as_ptr().add(i) };
        i += 1;
    }
    result
}

pub fn direct_scene_preflight(
    splat_count: usize,
    sh_degree: u8,
    limits: &wgpu::Limits,
) -> Result<DirectScenePreflight, DirectSceneError> {
    let splat_count =
        u64::try_from(splat_count).map_err(|_| DirectSceneError::ResourceSizeOverflow)?;
    let capacity = splat_count.max(1);
    let binding_limit = u64::from(limits.max_storage_buffer_binding_size);
    let buffer_limit = limits.max_buffer_size;
    let effective_limit = binding_limit.min(buffer_limit);

    let order_stride = std::mem::size_of::<u32>() as u64;
    let source_stride = std::mem::size_of::<GpuSurfaceSourceElem>() as u64;
    let degree = u64::from(sh_degree);
    let sh_stride = degree
        .checked_add(1)
        .and_then(|value| value.checked_mul(value))
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| value.checked_mul(3))
        .and_then(|value| value.checked_mul(std::mem::size_of::<f32>() as u64))
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;

    let order_bytes = capacity
        .checked_mul(order_stride)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    let source_bytes = capacity
        .checked_mul(source_stride)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    let sh_bytes = if sh_stride == 0 {
        std::mem::size_of::<f32>() as u64
    } else {
        splat_count
            .checked_mul(sh_stride)
            .ok_or(DirectSceneError::ResourceSizeOverflow)?
    };

    let requirements = [
        DirectSceneResourceRequirement {
            resource: DirectSceneResource::SortedIndices,
            required_bytes: order_bytes,
            limit_bytes: effective_limit,
            fits: order_bytes <= effective_limit,
        },
        DirectSceneResourceRequirement {
            resource: DirectSceneResource::Source,
            required_bytes: source_bytes,
            limit_bytes: effective_limit,
            fits: source_bytes <= effective_limit,
        },
        DirectSceneResourceRequirement {
            resource: DirectSceneResource::ShRest,
            required_bytes: sh_bytes,
            limit_bytes: effective_limit,
            fits: sh_bytes <= effective_limit,
        },
    ];

    let capacities = [
        (
            DirectSceneResource::SortedIndices,
            effective_limit / order_stride,
        ),
        (DirectSceneResource::Source, effective_limit / source_stride),
        (
            DirectSceneResource::ShRest,
            if sh_stride == 0 {
                u64::MAX
            } else {
                effective_limit / sh_stride
            },
        ),
        (DirectSceneResource::DrawInstances, u64::from(u32::MAX)),
    ];
    let (limiting_resource, max_direct_splats) = capacities
        .into_iter()
        .min_by_key(|(_, capacity)| *capacity)
        .expect("direct resource capacity list is non-empty");
    let fits = requirements.iter().all(|requirement| requirement.fits)
        && splat_count <= u64::from(u32::MAX);
    let path = if fits {
        DirectScenePath::Direct
    } else {
        DirectScenePath::ActiveAtlasRequired
    };
    let remediation = if fits {
        DirectSceneRemediation::None
    } else {
        DirectSceneRemediation::UseActiveAtlasOrReduce { max_direct_splats }
    };

    Ok(DirectScenePreflight {
        splat_count,
        sh_degree,
        path,
        effective_storage_binding_limit: binding_limit,
        effective_max_buffer_size: buffer_limit,
        requirements,
        limiting_resource,
        max_direct_splats,
        remediation,
    })
}

/// Whether the packed path can host a scene without a single attribute
/// storage-buffer binding (the direct-path Nandi failure mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedScenePath {
    PackedAtlas,
    /// Texture dimensions exceed the device 2D limit; Phase D paging required.
    PagingRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedScenePreflight {
    pub splat_count: u64,
    pub sh_degree: u8,
    pub path: PackedScenePath,
    pub hot_atlas_width: u32,
    pub hot_atlas_height: u32,
    pub sh_atlas_width: u32,
    pub sh_atlas_height: u32,
    pub max_texture_dimension_2d: u32,
    pub sorted_indices_bytes: u64,
    /// Exact bytes requested by the packed attribute resource descriptors.
    /// This excludes driver-private allocator padding, which wgpu does not expose.
    pub declared_attribute_resource_bytes: u64,
    /// Tightly packed hot-record storage bytes (`20 * splat_count`).
    pub hot_record_storage_bytes: u64,
    pub sorted_indices_fits_storage_binding: bool,
    pub hot_record_fits_storage_binding: bool,
    /// True when full degree-3 SH attributes are not placed in a storage binding
    /// (the direct-path Nandi failure mode). Hot records may use compact storage.
    pub attributes_avoid_storage_binding: bool,
}

/// Packed-path resource preflight without allocating scene data.
pub fn packed_scene_preflight(
    splat_count: usize,
    sh_degree: u8,
    max_texture_dimension_2d: u32,
    max_storage_buffer_binding_size: u64,
) -> Result<PackedScenePreflight, DirectSceneError> {
    let splat_count_u64 =
        u64::try_from(splat_count).map_err(|_| DirectSceneError::ResourceSizeOverflow)?;
    let (hot_w, hot_h) = atlas_dimensions(splat_count);
    let (sh_w, sh_h) = sh_sidecar_atlas_dimensions(splat_count);
    let max_dim = max_texture_dimension_2d.max(1);
    let textures_fit = hot_w <= max_dim && hot_h <= max_dim && sh_w <= max_dim && sh_h <= max_dim;
    let sorted_indices_bytes = splat_count_u64
        .max(1)
        .checked_mul(std::mem::size_of::<u32>() as u64)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    let hot_record_storage_bytes = splat_count_u64
        .max(1)
        .checked_mul(HOT_RECORD_BYTES as u64)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    let declared_attribute_resource_bytes =
        measured_hot_texture_bytes(splat_count) + measured_sh_sidecar_texture_bytes(splat_count);
    let storage_bindings_fit = sorted_indices_bytes <= max_storage_buffer_binding_size
        && hot_record_storage_bytes <= max_storage_buffer_binding_size;
    Ok(PackedScenePreflight {
        splat_count: splat_count_u64,
        sh_degree,
        path: if textures_fit && storage_bindings_fit {
            PackedScenePath::PackedAtlas
        } else {
            PackedScenePath::PagingRequired
        },
        hot_atlas_width: hot_w,
        hot_atlas_height: hot_h,
        sh_atlas_width: sh_w,
        sh_atlas_height: sh_h,
        max_texture_dimension_2d: max_dim,
        sorted_indices_bytes,
        declared_attribute_resource_bytes,
        hot_record_storage_bytes,
        sorted_indices_fits_storage_binding: sorted_indices_bytes
            <= max_storage_buffer_binding_size,
        hot_record_fits_storage_binding: hot_record_storage_bytes
            <= max_storage_buffer_binding_size,
        // SH sidecars stay out of storage bindings; hot records use compact storage.
        attributes_avoid_storage_binding: true,
    })
}

struct DirectSceneResources {
    sorted_indices_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: usize,
    sh_degree: u32,
    _source_buffer: wgpu::Buffer,
    _sh_rest_buffer: wgpu::Buffer,
}

impl DirectSceneResources {
    fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
    ) -> Result<Self, DirectSceneError> {
        let preflight = direct_scene_preflight(scene.len(), scene.sh_degree, &device.limits())?;
        if preflight.path != DirectScenePath::Direct {
            return Err(DirectSceneError::ResourceLimitExceeded(Box::new(preflight)));
        }
        let capacity = scene.len().max(1);
        let sorted_indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-direct-sorted-indices"),
            size: (capacity as u64) * (std::mem::size_of::<u32>() as u64),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-params"),
            contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let source_elems = make_surface_source_elems(scene, world_covariance_terms, alpha_values);
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-source"),
            contents: bytemuck::cast_slice(&source_elems),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let sh_rest_fallback = [0.0_f32];
        let sh_rest = scene.sh_rest.as_deref().unwrap_or(&sh_rest_fallback);
        let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-sh-rest"),
            contents: bytemuck::cast_slice(sh_rest),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-direct-bind-group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sorted_indices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: source_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: sh_rest_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            sorted_indices_buffer,
            params_buffer,
            bind_group,
            capacity,
            sh_degree: scene.sh_degree as u32,
            _source_buffer: source_buffer,
            _sh_rest_buffer: sh_rest_buffer,
        })
    }

    fn prepare(
        &self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        upload_order: bool,
    ) -> Result<u32, DirectSceneError> {
        if sorted_indices.len() > self.capacity {
            return Err(DirectSceneError::SortedIndexCapacityExceeded);
        }
        if upload_order && !sorted_indices.is_empty() {
            queue.write_buffer(
                &self.sorted_indices_buffer,
                0,
                bytemuck::cast_slice(sorted_indices),
            );
        }
        let instance_count = sorted_indices.len() as u32;
        let params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }
}

fn create_direct_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-direct-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn create_direct_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label("gsplat-direct-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/splat_surface_direct.wgsl").into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label("gsplat-direct-pipeline-layout"),
        bind_group_layouts: &[bind_group_layout],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: wgpu_label("gsplat-direct-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Error, PartialEq, Eq)]
enum GpuRasterError {
    #[error("no compatible wgpu adapter")]
    AdapterUnavailable,
    #[error("wgpu device creation failed")]
    DeviceCreation,
    #[error("invalid render dimensions")]
    InvalidDimensions,
    #[error(
        "render dimensions {width}x{height} exceed the device 2D texture limit {max_dimension}"
    )]
    DimensionsUnsupported {
        width: u32,
        height: u32,
        max_dimension: u32,
    },
    #[error("failed to read back render target")]
    ReadbackFailed,
    #[error("direct scene resource error: {0}")]
    DirectScene(#[from] DirectSceneError),
}

#[cfg(not(target_arch = "wasm32"))]
impl From<GpuRasterError> for RendererError {
    fn from(error: GpuRasterError) -> Self {
        match error {
            GpuRasterError::AdapterUnavailable => Self::GpuRasterizerUnavailable,
            GpuRasterError::DeviceCreation => Self::GpuDeviceCreation,
            GpuRasterError::InvalidDimensions => Self::InvalidConfig,
            GpuRasterError::DimensionsUnsupported {
                width,
                height,
                max_dimension,
            } => Self::GpuDimensionsUnsupported {
                width,
                height,
                max_dimension,
            },
            GpuRasterError::ReadbackFailed => Self::GpuReadback,
            GpuRasterError::DirectScene(error) => Self::DirectScene(error),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct GpuRasterizer {
    adapter_info: wgpu::AdapterInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    output_size: (u32, u32),
    max_texture_dimension_2d: u32,
    direct_pipeline: wgpu::RenderPipeline,
    direct_bind_group_layout: wgpu::BindGroupLayout,
    direct_scene: Option<DirectSceneResources>,
    packed_pipeline: wgpu::RenderPipeline,
    packed_bind_group_layout: wgpu::BindGroupLayout,
    packed_scene: Option<packed_gpu::PackedAtlasResources>,
    /// Last camera position used for packed hot-color refresh.
    packed_color_refresh_position: Option<[f32; 3]>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GpuRasterizer {
    fn create(config: &RendererConfig) -> Result<Self, GpuRasterError> {
        pollster::block_on(Self::create_async(config))
    }

    async fn create_async(config: &RendererConfig) -> Result<Self, GpuRasterError> {
        if config.width == 0 || config.height == 0 {
            return Err(GpuRasterError::InvalidDimensions);
        }

        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| GpuRasterError::AdapterUnavailable)?;

        let adapter_info = adapter.get_info();
        let required_limits = offscreen_device_limits(config, &adapter.limits())?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-render-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|_| GpuRasterError::DeviceCreation)?;

        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;

        let (output_texture, output_view) = create_output_target(
            &device,
            config.width,
            config.height,
            max_texture_dimension_2d,
        )?;
        let direct_bind_group_layout = create_direct_bind_group_layout(&device);
        let direct_pipeline =
            create_direct_pipeline(&device, &direct_bind_group_layout, RENDER_TARGET_FORMAT);
        let packed_bind_group_layout = packed_gpu::create_packed_bind_group_layout(&device);
        let packed_pipeline = packed_gpu::create_packed_pipeline(
            &device,
            &packed_bind_group_layout,
            RENDER_TARGET_FORMAT,
        );

        Ok(Self {
            adapter_info,
            device,
            queue,
            output_texture,
            output_view,
            output_size: (config.width, config.height),
            max_texture_dimension_2d,
            direct_pipeline,
            direct_bind_group_layout,
            direct_scene: None,
            packed_pipeline,
            packed_bind_group_layout,
            packed_scene: None,
            packed_color_refresh_position: None,
        })
    }

    fn clear_scene_resources(&mut self) {
        self.direct_scene = None;
        self.packed_scene = None;
        self.packed_color_refresh_position = None;
    }

    fn render_direct_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
    ) -> Result<(), GpuRasterError> {
        self.ensure_output_target(config.width, config.height)?;
        if self.direct_scene.is_none() {
            self.direct_scene = Some(DirectSceneResources::new(
                &self.device,
                &self.direct_bind_group_layout,
                scene,
                world_covariance_terms,
                alpha_values,
            )?);
        }
        let direct_scene = self
            .direct_scene
            .as_ref()
            .ok_or(GpuRasterError::DeviceCreation)?;
        let instance_count = direct_scene
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| GpuRasterError::DeviceCreation)?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-offscreen-direct-encoder"),
            });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("gsplat-offscreen-direct-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.direct_pipeline);
            rpass.set_bind_group(0, &direct_scene.bind_group, &[]);
            if instance_count > 0 {
                rpass.draw(0..6, 0..instance_count);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn render_packed_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
    ) -> Result<(), GpuRasterError> {
        self.ensure_output_target(config.width, config.height)?;
        let mut force_refresh = false;
        if self.packed_scene.is_none() {
            self.packed_scene = Some(packed_gpu::PackedAtlasResources::from_scene(
                &self.device,
                &self.queue,
                &self.packed_bind_group_layout,
                scene,
            )?);
            force_refresh = true;
            self.packed_color_refresh_position = None;
        }
        let position_key = packed_color_refresh_position_key(camera);
        let packed_scene = self
            .packed_scene
            .as_ref()
            .ok_or(GpuRasterError::DeviceCreation)?;
        let needs_refresh = force_refresh
            || packed_color_refresh_needed(
                self.packed_color_refresh_position,
                camera,
                packed_scene.bounds_min,
                packed_scene.bounds_extent,
            );
        // Design: color-refresh writes view-evaluated RGB into the hot record;
        // the main draw then reads only the hot streams.
        if needs_refresh {
            let queue = self.queue.clone();
            let packed_scene = self
                .packed_scene
                .as_mut()
                .ok_or(GpuRasterError::DeviceCreation)?;
            refresh_packed_hot_colors(&queue, packed_scene, scene, camera);
            self.packed_color_refresh_position = Some(position_key);
        }
        let instance_count = self
            .packed_scene
            .as_ref()
            .ok_or(GpuRasterError::DeviceCreation)?
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| GpuRasterError::DeviceCreation)?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-offscreen-packed-encoder"),
            });
        {
            let packed_scene = self
                .packed_scene
                .as_ref()
                .ok_or(GpuRasterError::DeviceCreation)?;
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("gsplat-offscreen-packed-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.packed_pipeline);
            rpass.set_bind_group(0, &packed_scene.bind_group, &[]);
            if instance_count > 0 {
                rpass.draw(0..packed_gpu::PACKED_QUAD_VERTEX_COUNT, 0..instance_count);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn readback_rgba8(&mut self) -> Result<Vec<u8>, GpuRasterError> {
        use std::sync::mpsc;

        let (width, height) = self.output_size;
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidDimensions);
        }

        let bytes_per_pixel = 4_u32;
        let unpadded_bytes_per_row = width.saturating_mul(bytes_per_pixel);
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align).saturating_mul(align);
        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("splat-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("splat-readback-encoder"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.output_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
        }

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

        match rx.recv() {
            Ok(Ok(())) => {}
            _ => return Err(GpuRasterError::ReadbackFailed),
        }

        let mapped = slice.get_mapped_range();
        let unpadded = unpadded_bytes_per_row as usize;
        let padded = padded_bytes_per_row as usize;
        let mut out = vec![0_u8; unpadded.saturating_mul(height as usize)];

        for row in 0..(height as usize) {
            let src_start = row * padded;
            let dst_start = row * unpadded;
            out[dst_start..dst_start + unpadded]
                .copy_from_slice(&mapped[src_start..src_start + unpadded]);
        }

        drop(mapped);
        readback.unmap();
        Ok(out)
    }

    fn ensure_output_target(&mut self, width: u32, height: u32) -> Result<(), GpuRasterError> {
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidDimensions);
        }

        if self.output_size == (width, height) {
            return Ok(());
        }

        let (texture, view) =
            create_output_target(&self.device, width, height, self.max_texture_dimension_2d)?;
        self.output_texture = texture;
        self.output_view = view;
        self.output_size = (width, height);
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn offscreen_device_limits(
    config: &RendererConfig,
    adapter_limits: &wgpu::Limits,
) -> Result<wgpu::Limits, GpuRasterError> {
    if config.width == 0 || config.height == 0 {
        return Err(GpuRasterError::InvalidDimensions);
    }

    let requested_dimension = config.width.max(config.height);
    if requested_dimension > adapter_limits.max_texture_dimension_2d {
        return Err(GpuRasterError::DimensionsUnsupported {
            width: config.width,
            height: config.height,
            max_dimension: adapter_limits.max_texture_dimension_2d,
        });
    }

    let mut required_limits = wgpu::Limits::downlevel_defaults();
    // The offscreen renderer can switch to PackedAtlas after device creation,
    // so retain the adapter's available 2D texture ceiling for later scene
    // sidecars instead of freezing the device to the render-target size.
    required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    if !required_limits.check_limits(adapter_limits) {
        return Err(GpuRasterError::DeviceCreation);
    }
    Ok(required_limits)
}

#[cfg(not(target_arch = "wasm32"))]
fn create_output_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    max_texture_dimension_2d: u32,
) -> Result<(wgpu::Texture, wgpu::TextureView), GpuRasterError> {
    if width == 0 || height == 0 {
        return Err(GpuRasterError::InvalidDimensions);
    }
    if width > max_texture_dimension_2d || height > max_texture_dimension_2d {
        return Err(GpuRasterError::DimensionsUnsupported {
            width,
            height,
            max_dimension: max_texture_dimension_2d,
        });
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: wgpu_label("splat-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RENDER_TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ok((texture, view))
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, ErrorCode, RenderMode, RendererConfig, SceneBuffers, Vec3f};

    use super::{
        DirectSceneError, DirectScenePath, DirectSceneRemediation, DirectSceneResource,
        PackedScenePath, Renderer, build_instances, direct_scene_preflight,
        ellipse_axes_from_covariance, packed_scene_preflight, project_covariance_to_ndc,
        quat_inverse,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use super::{GpuRasterError, offscreen_device_limits};

    fn build_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 0.5), Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![0.9, 0.8],
            scale_xyz: vec![[0.0, 0.0, 0.0], [0.2, 0.2, 0.2]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.1, 0.2, 0.3], [0.3, 0.2, 0.1]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn sorted_alpha_pipeline_builds_visible_gaussians() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let (instances, stats) = renderer.build_sorted_instances(&Camera::default()).unwrap();

        assert_eq!(stats.visible_count, 2);
        assert_eq!(stats.drawn_count, 2);
        assert_eq!(instances.len(), 2);
    }

    #[test]
    fn packed_atlas_offscreen_smoke_preserves_counts() {
        let config = RendererConfig {
            width: 64,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };
        let mut renderer = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping packed atlas GPU smoke; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();
        let stats = renderer.render_frame(&Camera::default()).unwrap();
        assert_eq!(stats.visible_count, 2);
        assert_eq!(stats.drawn_count, 2);
        let rgba = renderer.readback_rgba8().unwrap();
        assert!(
            rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "packed path must produce at least one non-transparent pixel"
        );
    }

    #[test]
    fn packed_vs_direct_count_parity_on_minimal_scene() {
        let config = RendererConfig {
            width: 64,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };
        let mut direct = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping packed-vs-direct parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        packed.set_geometry_path(super::GeometryPath::PackedAtlas);
        let scene = build_scene();
        direct.load_scene(scene.clone()).unwrap();
        packed.load_scene(scene).unwrap();
        let camera = Camera::default();
        let direct_stats = direct.render_frame(&camera).unwrap();
        let packed_stats = packed.render_frame(&camera).unwrap();
        assert_eq!(direct_stats.visible_count, packed_stats.visible_count);
        assert_eq!(direct_stats.drawn_count, packed_stats.drawn_count);
        assert_eq!(direct_stats.visible_count, 2);
    }

    #[derive(Debug, Clone, Copy)]
    struct ImageParityMetrics {
        mean_abs_rgb: f64,
        frac_pixels_over_3_255: f64,
        max_abs_rgb: f64,
    }

    fn rgba_image_parity_metrics(direct: &[u8], packed: &[u8]) -> ImageParityMetrics {
        assert_eq!(direct.len(), packed.len());
        assert_eq!(direct.len() % 4, 0);
        let pixels = direct.len() / 4;
        let mut sum = 0.0_f64;
        let mut pixels_over = 0_u64;
        let mut max_abs = 0.0_f64;
        for index in 0..pixels {
            let base = index * 4;
            let mut pixel_over = false;
            for channel in 0..3 {
                let a = f64::from(direct[base + channel]) / 255.0;
                let b = f64::from(packed[base + channel]) / 255.0;
                let err = (a - b).abs();
                sum += err;
                max_abs = max_abs.max(err);
                if err > 3.0 / 255.0 {
                    pixel_over = true;
                }
            }
            pixels_over += u64::from(pixel_over);
        }
        ImageParityMetrics {
            mean_abs_rgb: sum / ((pixels * 3) as f64).max(1.0),
            frac_pixels_over_3_255: (pixels_over as f64) / (pixels as f64).max(1.0),
            max_abs_rgb: max_abs,
        }
    }

    #[test]
    fn image_parity_threshold_counts_pixels_not_channels() {
        let direct = [0_u8, 0, 0, 255, 0, 0, 0, 255];
        let packed = [4_u8, 0, 0, 255, 0, 0, 0, 255];
        let metrics = rgba_image_parity_metrics(&direct, &packed);
        assert_eq!(metrics.frac_pixels_over_3_255, 0.5);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn orbit_camera_for_scene(scene: &SceneBuffers, config: RendererConfig, yaw: f32) -> Camera {
        let first = *scene.positions.first().expect("non-empty scene");
        let (mut min, mut max) = (first, first);
        for position in &scene.positions[1..] {
            min.x = min.x.min(position.x);
            min.y = min.y.min(position.y);
            min.z = min.z.min(position.z);
            max.x = max.x.max(position.x);
            max.y = max.y.max(position.y);
            max.z = max.z.max(position.z);
        }
        let center = Vec3f::new(
            (min.x + max.x) * 0.5,
            (min.y + max.y) * 0.5,
            (min.z + max.z) * 0.5,
        );
        let half_x = ((max.x - min.x) * 0.5).max(1e-3);
        let half_y = ((max.y - min.y) * 0.5).max(1e-3);
        let half_z = ((max.z - min.z) * 0.5).max(1e-3);
        let aspect = config.width as f32 / config.height.max(1) as f32;
        let vfov = Camera::default().intrinsics.vertical_fov_radians;
        let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();
        let distance =
            ((half_y / (vfov * 0.5).tan()).max(half_x / (hfov * 0.5).tan()) + half_z) * 1.2;
        let mut camera = Camera::default();
        camera.pose.position = Vec3f::new(
            center.x + yaw.sin() * distance,
            center.y,
            center.z - yaw.cos() * distance,
        );
        camera.pose.rotation_xyzw = [0.0, -(yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
        let radius = half_x.max(half_y).max(half_z);
        camera.intrinsics.near_plane = (distance - radius * 2.0).max(0.01);
        camera.intrinsics.far_plane = (distance + radius * 8.0).max(100.0);
        camera
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn assert_two_revision_stale_order_quality(scene: SceneBuffers, label: &str) {
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut reference = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping {label} stale-order quality; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        reference.set_geometry_path(super::GeometryPath::PackedAtlas);
        let mut stale = Renderer::with_config(config).expect("second renderer");
        stale.set_geometry_path(super::GeometryPath::PackedAtlas);
        let old_camera = orbit_camera_for_scene(&scene, config, 0.0);
        let current_camera = orbit_camera_for_scene(&scene, config, 0.002);
        reference.load_scene(scene.clone()).unwrap();
        stale.load_scene(scene).unwrap();

        let (fresh_order, _) = reference.build_sorted_indices(&current_camera).unwrap();
        let (stale_order, _) = stale.build_sorted_indices(&old_camera).unwrap();
        let mut fresh_visible_set = fresh_order.clone();
        let mut stale_visible_set = stale_order.clone();
        fresh_visible_set.sort_unstable();
        stale_visible_set.sort_unstable();
        assert_eq!(
            fresh_visible_set, stale_visible_set,
            "{label} visible set changed across the two-revision quality envelope"
        );

        reference
            .render_frame_with_external_order_for_test(&current_camera, &fresh_order)
            .unwrap();
        stale
            .render_frame_with_external_order_for_test(&current_camera, &stale_order)
            .unwrap();
        let metrics = rgba_image_parity_metrics(
            &reference.readback_rgba8().unwrap(),
            &stale.readback_rgba8().unwrap(),
        );
        eprintln!(
            "{label} two-revision stale-order parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.frac_pixels_over_3_255, metrics.max_abs_rgb
        );
        assert!(metrics.mean_abs_rgb <= 1.0 / 255.0);
        assert!(metrics.frac_pixels_over_3_255 <= 0.001);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    #[ignore = "temporal stale-order pixel-tail gate is not yet met; retained as a research oracle"]
    fn bounded_async_two_revision_order_passes_kitsune_and_flowers_quality() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets");
        let datasets = [
            (
                "Kitsune",
                root.join("external/wakufactory_kitune/kitune1.ply"),
            ),
            (
                "Flowers",
                root.join("external/nvidia_flowers_1/flowers_1/flowers_1.ply"),
            ),
        ];
        for (label, path) in datasets {
            if !path.is_file() {
                eprintln!("skipping {label} stale-order quality; dataset missing");
                continue;
            }
            let loaded = gsplat_io_ply::load_ply(&path)
                .unwrap_or_else(|error| panic!("load {label} at {}: {error}", path.display()));
            assert_two_revision_stale_order_quality(loaded.scene, label);
        }
    }

    #[test]
    fn packed_color_refresh_uses_scene_relative_translation_bound() {
        let bounds_min = [-1.0, -1.0, -1.0];
        let bounds_extent = [2.0, 2.0, 2.0];
        let previous = [0.0, 0.0, -5.0];
        let mut camera = Camera::default();
        camera.pose.position = Vec3f::new(0.5, 0.0, -4.97);
        assert!(!super::packed_color_refresh_needed(
            Some(previous),
            &camera,
            bounds_min,
            bounds_extent,
        ));
        camera.pose.position = Vec3f::new(0.8, 0.0, -4.93);
        assert!(super::packed_color_refresh_needed(
            Some(previous),
            &camera,
            bounds_min,
            bounds_extent,
        ));
    }

    #[test]
    fn packed_color_refresh_ignores_rotation_at_fixed_position() {
        let mut camera = Camera::default();
        let previous = super::packed_color_refresh_position_key(&camera);
        camera.pose.rotation_xyzw = [0.0, 0.707_106_77, 0.0, 0.707_106_77];
        assert!(!super::packed_color_refresh_needed(
            Some(previous),
            &camera,
            [-1.0; 3],
            [2.0; 3],
        ));
    }

    #[test]
    fn packed_renderer_does_not_retain_direct_cpu_covariance_caches() {
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();
        assert!(renderer.world_covariances.is_none());
        assert!(renderer.world_covariance_terms.is_none());
        assert!(renderer.alpha_values.is_none());

        renderer.set_geometry_path(super::GeometryPath::SortedIndexDirect);
        assert_eq!(renderer.world_covariances.as_ref().map(Vec::len), Some(2));
        assert_eq!(
            renderer.world_covariance_terms.as_ref().map(Vec::len),
            Some(2)
        );
        assert_eq!(renderer.alpha_values.as_ref().map(Vec::len), Some(2));

        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        assert!(renderer.world_covariances.is_none());
        assert!(renderer.world_covariance_terms.is_none());
        assert!(renderer.alpha_values.is_none());
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_degree0_scene() {
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut direct = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping packed image parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        packed.set_geometry_path(super::GeometryPath::PackedAtlas);
        let scene = build_scene();
        direct.load_scene(scene.clone()).unwrap();
        packed.load_scene(scene).unwrap();
        let camera = Camera::default();
        let direct_stats = direct.render_frame(&camera).unwrap();
        let packed_stats = packed.render_frame(&camera).unwrap();
        assert_eq!(direct_stats.visible_count, packed_stats.visible_count);
        assert_eq!(direct_stats.drawn_count, packed_stats.drawn_count);
        let direct_rgba = direct.readback_rgba8().unwrap();
        let packed_rgba = packed.readback_rgba8().unwrap();
        let metrics = rgba_image_parity_metrics(&direct_rgba, &packed_rgba);
        eprintln!(
            "packed image parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.frac_pixels_over_3_255, metrics.max_abs_rgb
        );
        // Provisional Phase B gate from verification_plan.md.
        assert!(
            metrics.mean_abs_rgb <= 1.0 / 255.0,
            "mean abs RGB {:.6} exceeded 1/255",
            metrics.mean_abs_rgb
        );
        assert!(
            metrics.frac_pixels_over_3_255 <= 0.001,
            "frac over 3/255 {:.6} exceeded 0.1%",
            metrics.frac_pixels_over_3_255
        );
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_minimal_ascii() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/minimal_ascii.ply");
        let loaded = gsplat_io_ply::load_ply(&path).expect("load minimal_ascii");
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut direct = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping minimal_ascii image parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        packed.set_geometry_path(super::GeometryPath::PackedAtlas);
        direct.load_scene(loaded.scene.clone()).unwrap();
        packed.load_scene(loaded.scene).unwrap();
        let camera = Camera::default();
        let direct_stats = direct.render_frame(&camera).unwrap();
        let packed_stats = packed.render_frame(&camera).unwrap();
        assert_eq!(direct_stats.visible_count, packed_stats.visible_count);
        assert_eq!(direct_stats.drawn_count, packed_stats.drawn_count);
        let metrics = rgba_image_parity_metrics(
            &direct.readback_rgba8().unwrap(),
            &packed.readback_rgba8().unwrap(),
        );
        eprintln!(
            "minimal_ascii packed parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.frac_pixels_over_3_255, metrics.max_abs_rgb
        );
        assert!(metrics.mean_abs_rgb <= 1.0 / 255.0);
        assert!(metrics.frac_pixels_over_3_255 <= 0.001);
    }

    fn scene_to_rdf_ply_for_spz_parity(scene: &SceneBuffers) -> String {
        // Mirror gsplat-io-spz attribute-gate authoring: emit RDF so PLY load
        // recovers the same RUF SceneBuffers as the SPZ fixture.
        let mut ply =
            String::from("ply\nformat ascii 1.0\ncomment paired SPZ/PLY offscreen image parity\n");
        ply.push_str(&format!("element vertex {}\n", scene.len()));
        for property in [
            "x", "y", "z", "opacity", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2",
            "rot_3", "f_dc_0", "f_dc_1", "f_dc_2",
        ] {
            ply.push_str("property float ");
            ply.push_str(property);
            ply.push('\n');
        }
        ply.push_str("end_header\n");
        for index in 0..scene.len() {
            let position = scene.positions[index];
            let scale = scene.scale_xyz[index];
            let rotation = scene.rotation_xyzw[index];
            let color = scene.color_dc[index];
            let ply_w = rotation[3];
            let ply_x = -rotation[0];
            let ply_y = rotation[1];
            let ply_z = -rotation[2];
            ply.push_str(&format!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                position.x,
                -position.y,
                position.z,
                scene.opacity[index],
                scale[0],
                scale[1],
                scale[2],
                ply_w,
                ply_x,
                ply_y,
                ply_z,
                color[0],
                color[1],
                color[2],
            ));
        }
        ply
    }

    fn frame_camera_for_scene(scene: &SceneBuffers, width: u32, height: u32) -> Camera {
        let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for position in &scene.positions {
            min.x = min.x.min(position.x);
            min.y = min.y.min(position.y);
            min.z = min.z.min(position.z);
            max.x = max.x.max(position.x);
            max.y = max.y.max(position.y);
            max.z = max.z.max(position.z);
        }
        let center = Vec3f::new(
            0.5 * (min.x + max.x),
            0.5 * (min.y + max.y),
            0.5 * (min.z + max.z),
        );
        let half_x = ((max.x - min.x) * 0.5).max(1.0e-3);
        let half_y = ((max.y - min.y) * 0.5).max(1.0e-3);
        let half_z = ((max.z - min.z) * 0.5).max(1.0e-3);
        let aspect = width as f32 / height.max(1) as f32;
        let mut camera = Camera::default();
        let vfov = camera.intrinsics.vertical_fov_radians.max(1.0e-3);
        let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();
        let dist_y = half_y / (vfov * 0.5).tan();
        let dist_x = half_x / (hfov * 0.5).tan();
        let distance = (dist_y.max(dist_x) + half_z) * 1.2;
        let radius = half_x.max(half_y).max(half_z);
        camera.pose.position = Vec3f::new(center.x, center.y, center.z - distance);
        camera.intrinsics.near_plane = (distance - radius * 2.0).max(0.01);
        camera.intrinsics.far_plane = (distance + radius * 8.0).max(100.0);
        camera
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ply_vs_spz_offscreen_image_parity_gate_on_minimal_fixture() {
        let spz_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/minimal_v4_degree0.spz");
        let spz = gsplat_io_spz::load_spz(&spz_path).expect("load minimal SPZ fixture");
        let ply = gsplat_io_ply::parse_ply_text(&scene_to_rdf_ply_for_spz_parity(&spz.scene))
            .expect("paired RDF PLY must parse");
        assert_eq!(ply.summary.gaussians, spz.summary.gaussians);
        assert_eq!(ply.scene.len(), spz.scene.len());

        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut from_spz = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping PLY-vs-SPZ image parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut from_ply = Renderer::with_config(config).expect("second renderer");
        from_spz.load_scene(spz.scene.clone()).unwrap();
        from_ply.load_scene(ply.scene).unwrap();
        let camera = frame_camera_for_scene(&spz.scene, config.width, config.height);
        let spz_ttff_started = std::time::Instant::now();
        let spz_stats = from_spz.render_frame(&camera).unwrap();
        let spz_ttff_ms = spz_ttff_started.elapsed().as_secs_f64() * 1_000.0;
        let ply_ttff_started = std::time::Instant::now();
        let ply_stats = from_ply.render_frame(&camera).unwrap();
        let ply_ttff_ms = ply_ttff_started.elapsed().as_secs_f64() * 1_000.0;
        assert!(
            spz_stats.visible_count > 0,
            "framed SPZ fixture must produce visible splats"
        );
        assert_eq!(spz_stats.visible_count, ply_stats.visible_count);
        assert_eq!(spz_stats.drawn_count, ply_stats.drawn_count);
        let metrics = rgba_image_parity_metrics(
            &from_spz.readback_rgba8().unwrap(),
            &from_ply.readback_rgba8().unwrap(),
        );
        eprintln!(
            "PLY-vs-SPZ minimal fixture parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6} visible={} spz_ttff_ms={:.3} ply_ttff_ms={:.3}",
            metrics.mean_abs_rgb,
            metrics.frac_pixels_over_3_255,
            metrics.max_abs_rgb,
            spz_stats.visible_count,
            spz_ttff_ms,
            ply_ttff_ms
        );
        assert!(
            metrics.mean_abs_rgb <= 1.0 / 255.0,
            "mean abs RGB {:.6} exceeded 1/255",
            metrics.mean_abs_rgb
        );
        assert!(
            metrics.frac_pixels_over_3_255 <= 0.001,
            "frac over 3/255 {:.6} exceeded 0.1%",
            metrics.frac_pixels_over_3_255
        );

        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/benchmarks/phase-c");
        std::fs::create_dir_all(&out_dir).unwrap();
        let out_path = out_dir.join("minimal-spz-vs-ply-ttff.json");
        let payload = format!(
            "{{\n  \"schema\": \"gsplat-phase-c-ttff/v1\",\n  \"dataset\": \"minimal_v4_degree0\",\n  \"width\": {},\n  \"height\": {},\n  \"visible\": {},\n  \"drawn\": {},\n  \"ttff_ms\": {{\n    \"spz_first_frame\": {:.6},\n    \"ply_first_frame\": {:.6}\n  }},\n  \"notes\": \"ttff_ms measures first SortedAlpha render_frame after load_scene; adapter-dependent\"\n}}\n",
            config.width,
            config.height,
            spz_stats.visible_count,
            spz_stats.drawn_count,
            spz_ttff_ms,
            ply_ttff_ms,
        );
        std::fs::write(&out_path, payload).unwrap();
        eprintln!("wrote {}", out_path.display());
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_synthetic_degree3() {
        let count = 64usize;
        let scene = SceneBuffers {
            positions: (0..count)
                .map(|i| {
                    let t = i as f32 / count as f32;
                    Vec3f::new(
                        (t - 0.5) * 2.0,
                        ((i % 8) as f32 - 3.5) * 0.15,
                        1.5 + (i / 8) as f32 * 0.05,
                    )
                })
                .collect(),
            opacity: vec![2.0; count],
            scale_xyz: vec![[-3.0, -3.0, -3.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: (0..count)
                .map(|i| [0.1 + (i % 5) as f32 * 0.05, -0.05, 0.2])
                .collect(),
            sh_degree: 3,
            sh_rest: Some(
                (0..count * 45)
                    .map(|i| ((i % 11) as f32 - 5.0) * 0.03)
                    .collect(),
            ),
        };
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut direct = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping synthetic degree3 parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        packed.set_geometry_path(super::GeometryPath::PackedAtlas);
        direct.load_scene(scene.clone()).unwrap();
        packed.load_scene(scene).unwrap();
        let camera = Camera::default();
        let _ = direct.render_frame(&camera).unwrap();
        let _ = packed.render_frame(&camera).unwrap();
        let metrics = rgba_image_parity_metrics(
            &direct.readback_rgba8().unwrap(),
            &packed.readback_rgba8().unwrap(),
        );
        eprintln!(
            "synthetic degree3 packed parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.frac_pixels_over_3_255, metrics.max_abs_rgb
        );
        assert!(metrics.mean_abs_rgb <= 1.0 / 255.0);
        assert!(metrics.frac_pixels_over_3_255 <= 0.001);
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_kitsune_degree3() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/external/wakufactory_kitune/kitune1.ply");
        if !path.is_file() {
            eprintln!("skipping kitsune image parity; dataset missing");
            return;
        }
        let loaded = gsplat_io_ply::load_ply(&path).expect("load kitsune");
        assert_eq!(loaded.scene.sh_degree, 3);
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut direct = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping kitsune image parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        packed.set_geometry_path(super::GeometryPath::PackedAtlas);
        direct.load_scene(loaded.scene.clone()).unwrap();
        packed.load_scene(loaded.scene).unwrap();
        let camera = Camera::default();
        let direct_stats = direct.render_frame(&camera).unwrap();
        let packed_stats = packed.render_frame(&camera).unwrap();
        assert_eq!(direct_stats.visible_count, packed_stats.visible_count);
        assert_eq!(direct_stats.drawn_count, packed_stats.drawn_count);
        let direct_rgba = direct.readback_rgba8().unwrap();
        let packed_rgba = packed.readback_rgba8().unwrap();
        let metrics = rgba_image_parity_metrics(&direct_rgba, &packed_rgba);
        // Also report alpha-channel MAE to separate coverage vs color error.
        let mut sum_a = 0.0_f64;
        let mut n = 0_u64;
        for (d, p) in direct_rgba.chunks_exact(4).zip(packed_rgba.chunks_exact(4)) {
            sum_a += (d[3] as f64 - p[3] as f64).abs() / 255.0;
            n += 1;
        }
        let mut sum_direct_a = 0.0_f64;
        let mut sum_packed_a = 0.0_f64;
        for (d, p) in direct_rgba.chunks_exact(4).zip(packed_rgba.chunks_exact(4)) {
            sum_direct_a += d[3] as f64 / 255.0;
            sum_packed_a += p[3] as f64 / 255.0;
        }
        eprintln!(
            "kitsune packed parity: mean_abs_rgb={:.6} mean_abs_a={:.6} mean_a_direct={:.6} mean_a_packed={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6} visible={}",
            metrics.mean_abs_rgb,
            sum_a / n as f64,
            sum_direct_a / n as f64,
            sum_packed_a / n as f64,
            metrics.frac_pixels_over_3_255,
            metrics.max_abs_rgb,
            direct_stats.visible_count
        );
        assert!(
            metrics.mean_abs_rgb <= 1.0 / 255.0,
            "kitsune degree-3 MAE gate failed: mean_abs_rgb={:.6}",
            metrics.mean_abs_rgb
        );
        assert!(
            metrics.frac_pixels_over_3_255 <= 0.001,
            "kitsune degree-3 frac over 3/255 {:.6} exceeded 0.1%",
            metrics.frac_pixels_over_3_255
        );
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_flowers_degree3() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply");
        if !path.is_file() {
            eprintln!("skipping Flowers image parity; dataset missing");
            return;
        }
        let loaded = gsplat_io_ply::load_ply(&path).expect("load Flowers");
        assert_eq!(loaded.scene.sh_degree, 3);
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut direct = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping Flowers image parity; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        packed.set_geometry_path(super::GeometryPath::PackedAtlas);
        direct.load_scene(loaded.scene.clone()).unwrap();
        packed.load_scene(loaded.scene).unwrap();
        let camera = Camera::default();
        let direct_stats = direct.render_frame(&camera).unwrap();
        let packed_stats = packed.render_frame(&camera).unwrap();
        assert_eq!(direct_stats.visible_count, packed_stats.visible_count);
        assert_eq!(direct_stats.drawn_count, packed_stats.drawn_count);
        assert!(
            direct_stats.visible_count > 0,
            "Flowers parity camera must see splats"
        );
        let metrics = rgba_image_parity_metrics(
            &direct.readback_rgba8().unwrap(),
            &packed.readback_rgba8().unwrap(),
        );
        eprintln!(
            "Flowers packed parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6} visible={}",
            metrics.mean_abs_rgb,
            metrics.frac_pixels_over_3_255,
            metrics.max_abs_rgb,
            direct_stats.visible_count
        );
        assert!(
            metrics.mean_abs_rgb <= 1.0 / 255.0,
            "Flowers degree-3 MAE gate failed: mean_abs_rgb={:.6}",
            metrics.mean_abs_rgb
        );
        assert!(
            metrics.frac_pixels_over_3_255 <= 0.001,
            "Flowers degree-3 frac over 3/255 {:.6} exceeded 0.1%",
            metrics.frac_pixels_over_3_255
        );
    }

    #[test]
    fn sorted_alpha_orders_visible_indices_back_to_front() {
        let scene = SceneBuffers {
            positions: vec![
                Vec3f::new(0.0, 0.0, 0.5),
                Vec3f::new(0.0, 0.0, 2.0),
                Vec3f::new(0.0, 0.0, 1.0),
                Vec3f::new(0.0, 0.0, -1.0),
                Vec3f::new(0.0, 0.0, 2000.0),
            ],
            opacity: vec![1.0; 5],
            scale_xyz: vec![[0.0, 0.0, 0.0]; 5],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 5],
            color_dc: vec![[0.2, 0.3, 0.4]; 5],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(scene).unwrap();

        let stats = renderer
            .build_surface_sorted_indices_with_sort_refresh(&Camera::default(), true)
            .unwrap();

        assert_eq!(stats.visible_count, 3);
        assert_eq!(stats.drawn_count, 3);
        assert_eq!(renderer.current_sorted_indices(), &[1, 2, 0]);
    }

    #[test]
    fn preprocess_rejects_missing_scene() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        let err = renderer.preprocess_visible(&Camera::default()).unwrap_err();
        assert_eq!(
            err.code() as i32,
            gsplat_core::ErrorCode::SceneNotLoaded as i32
        );
    }

    #[test]
    fn preprocess_rejects_invalid_camera() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();
        let mut camera = Camera::default();
        camera.intrinsics.vertical_fov_radians = 0.0;

        let err = renderer.preprocess_visible(&camera).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[test]
    fn quaternion_inverse_normalizes_scaled_input() {
        assert_eq!(quat_inverse([0.0, 0.0, 0.0, 2.0]), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn surface_renderer_constructs_without_offscreen_gpu() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        assert!(!renderer.has_gpu_rasterizer());
    }

    #[test]
    fn direct_scene_preflight_accessor_requires_a_loaded_scene() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();

        let error = renderer.current_direct_scene_preflight().unwrap_err();

        assert!(matches!(error, super::RendererError::SceneNotLoaded));
    }

    #[test]
    fn direct_scene_preflight_accessor_does_not_guess_surface_device_limits() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let error = renderer.current_direct_scene_preflight().unwrap_err();

        assert!(matches!(
            error,
            super::RendererError::GpuRasterizerUnavailable
        ));
    }

    #[test]
    fn surface_renderer_rejects_offscreen_render() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let err = renderer.render_frame(&Camera::default()).unwrap_err();

        assert!(matches!(
            err,
            super::RendererError::GpuRasterizerUnavailable
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_reject_unsupported_dimensions_before_device_creation() {
        let adapter_limits = wgpu::Limits::downlevel_defaults();
        let config = RendererConfig {
            width: 4096,
            height: 2160,
            mode: RenderMode::SortedAlpha,
        };

        let err = offscreen_device_limits(&config, &adapter_limits).unwrap_err();

        assert_eq!(
            err,
            GpuRasterError::DimensionsUnsupported {
                width: 4096,
                height: 2160,
                max_dimension: 2048,
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_preserve_adapter_texture_dimension_for_later_packed_scenes() {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_texture_dimension_2d = 8192;
        let config = RendererConfig {
            width: 4096,
            height: 2160,
            mode: RenderMode::SortedAlpha,
        };

        let requested = offscreen_device_limits(&config, &adapter_limits).unwrap();

        assert_eq!(requested.max_texture_dimension_2d, 8192);
    }

    fn limits_with_storage_binding_limit(bytes: u32) -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffer_binding_size = bytes;
        limits.max_buffer_size = u64::from(bytes);
        limits
    }

    #[test]
    fn direct_scene_preflight_accounts_for_empty_scene_fallback_buffers() {
        let report =
            direct_scene_preflight(0, 0, &limits_with_storage_binding_limit(128 * 1024 * 1024))
                .unwrap();

        assert_eq!(report.path, DirectScenePath::Direct);
        assert_eq!(report.requirements[0].required_bytes, 4);
        assert_eq!(report.requirements[1].required_bytes, 64);
        assert_eq!(report.requirements[2].required_bytes, 4);
    }

    #[test]
    fn direct_scene_preflight_enforces_source_binding_boundary() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let at_limit = direct_scene_preflight(2_097_152, 0, &limits).unwrap();
        let above_limit = direct_scene_preflight(2_097_153, 0, &limits).unwrap();

        assert_eq!(at_limit.path, DirectScenePath::Direct);
        assert_eq!(at_limit.limiting_resource, DirectSceneResource::Source);
        assert_eq!(above_limit.path, DirectScenePath::ActiveAtlasRequired);
        assert_eq!(above_limit.requirements[1].required_bytes, 134_217_792);
        assert_eq!(
            above_limit.remediation,
            DirectSceneRemediation::UseActiveAtlasOrReduce {
                max_direct_splats: 2_097_152,
            }
        );
    }

    #[test]
    fn direct_scene_preflight_enforces_degree_three_sh_boundary() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let at_limit = direct_scene_preflight(745_654, 3, &limits).unwrap();
        let above_limit = direct_scene_preflight(745_655, 3, &limits).unwrap();

        assert_eq!(at_limit.path, DirectScenePath::Direct);
        assert_eq!(at_limit.limiting_resource, DirectSceneResource::ShRest);
        assert_eq!(above_limit.path, DirectScenePath::ActiveAtlasRequired);
        assert_eq!(above_limit.requirements[2].required_bytes, 134_217_900);
    }

    #[test]
    fn direct_scene_preflight_reports_nandi_without_allocating_scene_data() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let dc = direct_scene_preflight(3_454_040, 0, &limits).unwrap();
        let degree_three = direct_scene_preflight(3_454_040, 3, &limits).unwrap();

        assert_eq!(dc.path, DirectScenePath::ActiveAtlasRequired);
        assert_eq!(dc.limiting_resource, DirectSceneResource::Source);
        assert_eq!(dc.requirements[1].required_bytes, 221_058_560);
        assert_eq!(degree_three.path, DirectScenePath::ActiveAtlasRequired);
        assert_eq!(degree_three.limiting_resource, DirectSceneResource::ShRest);
        assert_eq!(degree_three.requirements[2].required_bytes, 621_727_200);
        assert!(!degree_three.requirements[1].fits);
        assert!(!degree_three.requirements[2].fits);
    }

    #[test]
    fn packed_scene_preflight_removes_nandi_attribute_binding_failure() {
        let binding_limit = 128 * 1024 * 1024_u64;
        let max_dim = 8192_u32;
        let kitsune = packed_scene_preflight(279_199, 3, max_dim, binding_limit).unwrap();
        let nandi = packed_scene_preflight(3_454_040, 3, max_dim, binding_limit).unwrap();
        let direct_nandi = direct_scene_preflight(
            3_454_040,
            3,
            &limits_with_storage_binding_limit(128 * 1024 * 1024),
        )
        .unwrap();

        // Direct Nandi fails because SH rest alone exceeds the storage binding.
        assert_eq!(direct_nandi.path, DirectScenePath::ActiveAtlasRequired);
        assert_eq!(direct_nandi.limiting_resource, DirectSceneResource::ShRest);
        assert!(!direct_nandi.requirements[2].fits);

        // Packed keeps full degree-3 SH out of storage bindings (direct Nandi failure).
        // Hot records use a compact 20 B/splat storage buffer that still fits Nandi.
        assert!(kitsune.attributes_avoid_storage_binding);
        assert!(nandi.attributes_avoid_storage_binding);
        assert!(kitsune.hot_record_fits_storage_binding);
        assert!(nandi.hot_record_fits_storage_binding);
        assert_eq!(
            nandi.hot_record_storage_bytes,
            3_454_040_u64 * 20,
            "Nandi hot storage must stay under the 128 MiB binding limit"
        );
        assert!(kitsune.sorted_indices_fits_storage_binding);
        assert!(nandi.sorted_indices_fits_storage_binding);
        assert_eq!(kitsune.path, PackedScenePath::PackedAtlas);
        // Nandi SH atlas height exceeds common 8K 2D limits → paging (Phase D),
        // not a storage-binding failure.
        assert_eq!(nandi.path, PackedScenePath::PagingRequired);
        assert!(nandi.sh_atlas_height > max_dim);
        assert!(
            nandi.declared_attribute_resource_bytes
                < direct_nandi.requirements[1].required_bytes
                    + direct_nandi.requirements[2].required_bytes
        );
        let reduction = (direct_nandi.requirements[1].required_bytes
            + direct_nandi.requirements[2].required_bytes) as f64
            / nandi.declared_attribute_resource_bytes as f64;
        assert!(
            reduction >= 3.0,
            "packed Nandi attribute bytes should be ≥3x smaller than direct source+SH: {reduction}"
        );
    }

    #[test]
    fn packed_scene_preflight_accepts_kitsune_and_flowers_on_mobile_limits() {
        let binding_limit = 128 * 1024 * 1024_u64;
        // Mobile downlevel defaults commonly expose 4096 or 8192.
        for max_dim in [4096_u32, 8192_u32] {
            for count in [279_199_usize, 562_974_usize] {
                let report = packed_scene_preflight(count, 3, max_dim, binding_limit).unwrap();
                assert_eq!(report.path, PackedScenePath::PackedAtlas);
                assert!(report.attributes_avoid_storage_binding);
                assert!(report.hot_record_fits_storage_binding);
                assert!(report.sorted_indices_fits_storage_binding);
                assert!(report.hot_atlas_height <= max_dim);
                assert!(report.sh_atlas_height <= max_dim);
            }
        }
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn direct_scene_preflight_rejects_more_than_u32_draw_instances() {
        let count = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
        let report = direct_scene_preflight(count, 0, &wgpu::Limits::default()).unwrap();

        assert_eq!(report.path, DirectScenePath::ActiveAtlasRequired);
        assert!(report.splat_count > u64::from(u32::MAX));
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn direct_scene_preflight_rejects_byte_arithmetic_overflow() {
        let error = direct_scene_preflight(usize::MAX, 3, &wgpu::Limits::default()).unwrap_err();

        assert_eq!(error, DirectSceneError::ResourceSizeOverflow);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn four_k_offscreen_construction_never_unwinds() {
        let config = RendererConfig {
            width: 4096,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };

        let result = std::panic::catch_unwind(|| Renderer::with_config(config));

        assert!(result.is_ok(), "4K construction must return a Result");
        if let Err(error) = result.unwrap() {
            assert!(matches!(
                error,
                super::RendererError::GpuRasterizerUnavailable
                    | super::RendererError::GpuDeviceCreation
                    | super::RendererError::GpuDimensionsUnsupported { .. }
            ));
        }
    }

    #[test]
    fn covariance_projection_produces_nonzero_ellipse_axes() {
        let camera = Camera::default();
        let config = RendererConfig::default();
        let cov_cam = [
            [0.020, 0.000, 0.000],
            [0.000, 0.005, 0.000],
            [0.000, 0.000, 0.002],
        ];
        let cov2 = project_covariance_to_ndc(Vec3f::new(0.2, -0.1, 2.0), cov_cam, &camera, config)
            .expect("covariance should project");
        let (axis_u, axis_v) =
            ellipse_axes_from_covariance(cov2).expect("ellipse axes should be finite");

        let lu = (axis_u[0] * axis_u[0] + axis_u[1] * axis_u[1]).sqrt();
        let lv = (axis_v[0] * axis_v[0] + axis_v[1] * axis_v[1]).sqrt();
        assert!(lu > 0.0);
        assert!(lv > 0.0);
        assert!(lu > lv);
    }

    #[test]
    fn build_instances_generates_anisotropic_oriented_axes() {
        let qz = 0.5_f32.sqrt(); // sin/cos(90deg / 2)
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[0.8, -0.4, -0.4]],
            rotation_xyzw: vec![[0.0, 0.0, qz, qz]],
            color_dc: vec![[0.2, 0.3, 0.4]],
            sh_degree: 0,
            sh_rest: None,
        };

        let world_cov = super::precompute_world_covariances(&scene);
        let alpha_values = super::precompute_alpha_values(&scene);
        let instances = build_instances(
            &scene,
            &world_cov,
            &alpha_values,
            &[0],
            &Camera::default(),
            RendererConfig::default(),
        );
        assert_eq!(instances.len(), 1);
        let inst = instances[0];
        let axis_u = [inst.center_and_axis_u[2], inst.center_and_axis_u[3]];
        let axis_v = [inst.axis_v_and_pad[0], inst.axis_v_and_pad[1]];

        let lu = (axis_u[0] * axis_u[0] + axis_u[1] * axis_u[1]).sqrt();
        let lv = (axis_v[0] * axis_v[0] + axis_v[1] * axis_v[1]).sqrt();
        let dot = axis_u[0] * axis_v[0] + axis_u[1] * axis_v[1];
        assert!(lu > lv);
        assert!(dot.abs() < 1e-4);
    }

    #[test]
    fn build_instances_keeps_partial_splats_with_offscreen_center() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(2.3, 0.0, 1.0)],
            opacity: vec![1.0],
            // Large sigma to ensure the projected ellipse overlaps the viewport.
            scale_xyz: vec![[2.0, 2.0, 2.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.2, 0.3, 0.4]],
            sh_degree: 0,
            sh_rest: None,
        };

        let world_cov = super::precompute_world_covariances(&scene);
        let alpha_values = super::precompute_alpha_values(&scene);
        let instances = build_instances(
            &scene,
            &world_cov,
            &alpha_values,
            &[0],
            &Camera::default(),
            RendererConfig::default(),
        );
        assert_eq!(instances.len(), 1);

        let inst = instances[0];
        let center_x = inst.center_and_axis_u[0];
        let extent_x = inst.center_and_axis_u[2].abs() + inst.axis_v_and_pad[0].abs();
        assert!(center_x > 2.0);
        assert!(center_x - extent_x <= 1.0);
    }
}
