#![deny(clippy::debug_assert_with_mut_call)]

//! WGPU renderer with a SortedAlpha reference path.

mod api;
mod cpu;
mod cpu_order;
mod data;
mod direct_gpu_order;
mod evidence;
#[cfg_attr(not(test), allow(dead_code))]
mod gpu;
mod gpu_error;
mod gpu_producer_telemetry;
mod gpu_telemetry;
#[cfg(not(target_arch = "wasm32"))]
mod offscreen;
mod packed_atlas;
mod packed_gpu;
mod page_atlas;
mod page_scheduler;
mod page_source;
mod paged_active_set;
mod paged_gpu;
#[cfg_attr(not(test), allow(dead_code))]
mod plans;
#[cfg_attr(not(test), allow(dead_code))]
mod preproject_gpu;
mod projected_draw_telemetry;
mod projected_quads_gpu;
mod raster;
#[cfg_attr(not(test), allow(dead_code))]
mod renderer;
mod residency;
mod resident_gpu;
mod scene;
mod spatial_pages;
mod surface;
mod surface_presenter;
mod surface_session;
mod tiled_resident_gpu;

pub use api::{GeometryPath, PreprocessOutput};
use cpu_order::CpuOrderEngine;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use cpu_order::{
    PARALLEL_PREPROCESS_THRESHOLD, preprocess_positions_visible_into_parallel,
};
#[cfg(test)]
pub(crate) use cpu_order::{depth_to_key, world_to_camera_depth_with_view_row};
pub(crate) use cpu_order::{
    is_visible, preprocess_paged_visible_into, preprocess_positions_visible_into,
};
pub(crate) use data::{
    CameraCovarianceTerms, CpuPositionView, GpuSortPair, GpuSurfaceRenderParams,
    GpuSurfaceSourceElem, ShColorLayout, SplatSetView,
};
pub use data::{
    GpuInstance, RESIDENT_CHUNK_META_BYTES, RESIDENT_CHUNK_SPLATS, RESIDENT_COLOR_AUX_WORDS,
    RESIDENT_COVARIANCE0_FLOATS, RESIDENT_COVARIANCE1_FLOATS, RESIDENT_SH_PLANES,
    RESIDENT_SH_WORDS_PER_PLANE, ResidentChunkMeta, ResidentColorAux, ResidentCovariance0,
    ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};
pub use evidence::{
    SurfaceCompatibilityChannel, SurfaceCompatibilityCountFamily, SurfaceCompatibilityCounts,
    SurfaceCompatibilityCountsTake, SurfaceCompatibilityCountsUnavailable,
    SurfaceCompatibilityCountsUnavailableReason, SurfaceCompatibilityOrderCpuSuccess,
    SurfaceCompatibilityOrderFailure, SurfaceCompatibilityOrderGpuSuccess,
    SurfaceCompatibilityOrderIssueContext, SurfaceCompatibilityOrderSubmission,
    SurfaceCompatibilityProducerFailure, SurfaceCompatibilityProducerIssueContext,
    SurfaceCompatibilityProducerSubmission, SurfaceCompatibilityProducerSuccess,
    SurfaceCompatibilityProjectedFailure, SurfaceCompatibilityProjectedIssueContext,
    SurfaceCompatibilityProjectedSubmission, SurfaceCompatibilityProjectedSuccess,
    SurfaceCompatibilitySubmission, SurfaceCompatibilityTerminal, SurfaceCompatibilityTerminalPoll,
    SurfaceCompatibilityTerminalSelector, SurfaceCompatibilityTerminalUnavailable,
};
pub use gpu_error::ResidentGpuError;
pub use gpu_producer_telemetry::{
    SurfaceGpuOrderProducer, SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
};
pub use gpu_telemetry::{
    SurfaceCpuOrderMeasurement, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfaceOrderMeasurementFailureReason, SurfaceTimingSource,
};
pub use packed_atlas::{
    DEGREE3_SIDECAR_BYTES, DIRECT_DEGREE3_ATTRIBUTE_BYTES, FULL_DEGREE3_ATTRIBUTE_BYTES,
    HOT_RECORD_BYTES, HotStream, LogScaleRange, PackedAtlasCpuBuffers, PackedHotRecord,
    PackedSceneCpu, PackedShSidecar, SceneBounds, atlas_dimensions, decode_opacity_u8,
    dequantize_sh_rest, measured_hot_texture_bytes, measured_sh_sidecar_texture_bytes,
    pack_color_rgb10, pack_quat_smallest_three, pack_scene, pack_scene_with_encoding,
    sh_sidecar_atlas_dimensions, slot_to_texel, unpack_color_rgb10, unpack_quat_smallest_three,
};
pub(crate) use page_scheduler::{SchedulerConfig, SchedulerView, schedule_pages};
pub(crate) use paged_gpu::PagedAtlasGpu;
pub use projected_draw_telemetry::{
    SurfaceProjectedDrawExecution, SurfaceProjectedDrawMeasurement,
    SurfaceProjectedDrawMeasurementFailure, SurfaceProjectedDrawMeasurementFailureReason,
};
pub(crate) use residency::{AttributeLod, ResidencyBudgets, ResidencyManager};
pub use scene::{
    DirectSceneError, DirectScenePath, DirectScenePreflight, DirectSceneRemediation,
    DirectSceneResource, DirectSceneResourceRequirement, PackedScenePath, PackedScenePreflight,
    PackedScenePreflightFailure, PackedScenePreflightLimits, ResidentCpuByteAccounting,
    ResidentEncodingReport, ResidentGpuBytePlan, ResidentSceneBuilder, ResidentSceneCpu,
    ResidentSceneError, ResidentSourceSplat, direct_scene_preflight, packed_scene_preflight,
    packed_scene_preflight_with_limits, resident_sh_plane_count,
};
pub(crate) use spatial_pages::{DEFAULT_PAGE_CAPACITY, SpatialPageSet};
pub use surface::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsCounts, SurfaceCurrentStatsFailure,
    SurfaceCurrentStatsFrameIdentity, SurfaceCurrentStatsJoinIdentity, SurfaceCurrentStatsPlan,
    SurfaceCurrentStatsPoll, SurfaceCurrentStatsReceipt, SurfaceCurrentStatsRequest,
    SurfaceCurrentStatsSubmission, SurfaceCurrentStatsSubmissionReceipt,
    SurfaceCurrentStatsTerminal, SurfaceCurrentStatsUnsampledReason,
};
pub use surface_presenter::{SurfaceFrameCapture, SurfacePresenter};
#[cfg(test)]
use surface_presenter::{SurfacePagedRuntime, surface_resource_plan, try_prepare_then_commit};
pub use surface_session::{
    SurfaceAdaptiveGpuFailureReason, SurfaceAdaptivePendingSample, SurfaceAdaptiveState,
    SurfaceFrameOutput, SurfaceFrameTimings, SurfaceGpuProducerMeasurementSubmission,
    SurfaceGpuProducerMeasurementUnsampledReason, SurfaceOrderBackend, SurfaceOrderBackendUsed,
    SurfaceOrderMeasurementSubmission, SurfaceOrderMeasurementUnsampledReason,
    SurfaceProjectedDrawAdaptivePendingSample, SurfaceProjectedDrawAdaptiveState,
    SurfaceProjectedDrawMeasurementSubmission, SurfaceProjectedDrawMeasurementUnsampledReason,
    SurfaceProjectedDrawPolicy, SurfaceRenderSession, SurfaceSortSchedule,
};
pub use tiled_resident_gpu::{ResidentTiledError, SurfaceRasterExecutionPlan};

const DEFAULT_PAGED_ATLAS_SLOTS: usize = 4;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use bytemuck::Zeroable;
use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_sort::SortError;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[cfg(not(target_arch = "wasm32"))]
const RENDER_TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type TimerInstant = Instant;

#[cfg(target_arch = "wasm32")]
pub(crate) type TimerInstant = f64;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn timer_now() -> TimerInstant {
    Instant::now()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn timer_now() -> TimerInstant {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or_else(js_sys::Date::now)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn timer_elapsed_ms(start: TimerInstant) -> f32 {
    start.elapsed().as_secs_f32() * 1000.0
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn timer_elapsed_ms(start: TimerInstant) -> f32 {
    (timer_now() - start).max(0.0) as f32
}

#[cfg(target_os = "android")]
pub(crate) const fn wgpu_label(_label: &'static str) -> Option<&'static str> {
    None
}

#[cfg(not(target_os = "android"))]
pub(crate) const fn wgpu_label(label: &'static str) -> Option<&'static str> {
    Some(label)
}

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("invalid renderer configuration")]
    InvalidConfig,
    #[error("invalid camera")]
    InvalidCamera,
    #[error("scene not loaded")]
    SceneNotLoaded,
    #[error("geometry path {path:?} needs source data that is unavailable after Packed GPU upload")]
    GeometrySourceUnavailable { path: GeometryPath },
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
    #[error("resident scene encoding error: {0}")]
    ResidentScene(#[from] ResidentSceneError),
    #[error("resident GPU resource error: {0}")]
    ResidentGpu(#[from] ResidentGpuError),
    #[error("resident exact tiled raster error: {0}")]
    ResidentTiled(#[from] ResidentTiledError),
    #[error("paged atlas error: {0:?}")]
    PagedAtlas(String),
    #[error("sort backend error: {0}")]
    Sort(#[from] SortError),
    #[error("surface presenter error: {0}")]
    SurfacePresenter(#[from] SurfacePresenterError),
}

impl RendererError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidConfig
            | Self::InvalidCamera
            | Self::InvalidScene
            | Self::ResidentScene(ResidentSceneError::CountMismatch { .. })
            | Self::ResidentScene(ResidentSceneError::InvalidSplat { .. }) => {
                ErrorCode::InvalidArgument
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::ResidentGpu(ResidentGpuError::GpuOrderInternal(_)) => ErrorCode::Internal,
            Self::ResidentTiled(ResidentTiledError::Internal(_))
            | Self::ResidentTiled(ResidentTiledError::Validation(_))
            | Self::ResidentTiled(ResidentTiledError::Readback)
            | Self::ResidentTiled(ResidentTiledError::IncompleteScatter)
            | Self::ResidentTiled(ResidentTiledError::OutOfMemory) => ErrorCode::Internal,
            Self::GeometrySourceUnavailable { .. } => ErrorCode::Unsupported,
            Self::GpuRasterizerUnavailable
            | Self::GpuDeviceCreation
            | Self::GpuDimensionsUnsupported { .. }
            | Self::DirectScene(DirectSceneError::ResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::PackedResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::PagedAtlasResourceLimitExceeded { .. })
            | Self::DirectScene(DirectSceneError::ResourceSizeOverflow)
            | Self::ResidentScene(ResidentSceneError::UnsupportedShDegree(_))
            | Self::ResidentScene(ResidentSceneError::SizeOverflow)
            | Self::ResidentScene(ResidentSceneError::UploadStagingReleased)
            | Self::ResidentGpu(_)
            | Self::ResidentTiled(_) => ErrorCode::Unsupported,
            Self::GpuReadback | Self::GpuWait | Self::SurfaceWorker => ErrorCode::Internal,
            Self::DirectScene(DirectSceneError::SortedIndexCapacityExceeded)
            | Self::DirectScene(DirectSceneError::GpuOrderInitialization(_))
            | Self::ResidentScene(ResidentSceneError::InvalidScene)
            | Self::ResidentScene(ResidentSceneError::AllocationFailed { .. })
            | Self::PagedAtlas(_) => ErrorCode::Internal,
            Self::Sort(_) => ErrorCode::Internal,
            Self::SurfacePresenter(err) => err.code(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn map_prepared_gpu_runtime_error(error: renderer::PreparedGpuRuntimeError) -> RendererError {
    use renderer::{GpuRuntimePreparationError, PreparedGpuRuntimeError, PreparedRuntimeError};

    match error {
        PreparedGpuRuntimeError::Runtime(PreparedRuntimeError::Scene(error)) => {
            RendererError::ResidentScene(error)
        }
        PreparedGpuRuntimeError::Runtime(
            PreparedRuntimeError::SourceCountOverflow | PreparedRuntimeError::ContractMismatch,
        ) => RendererError::InvalidScene,
        PreparedGpuRuntimeError::Runtime(
            PreparedRuntimeError::PlanSet(_) | PreparedRuntimeError::Generation(_),
        )
        | PreparedGpuRuntimeError::Gpu(GpuRuntimePreparationError::PlanSet(_))
        | PreparedGpuRuntimeError::Gpu(GpuRuntimePreparationError::Generation(_)) => {
            RendererError::GpuDeviceCreation
        }
        PreparedGpuRuntimeError::Gpu(GpuRuntimePreparationError::Resources(error)) => {
            map_gpu_preparation_error(error)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn map_gpu_preparation_error(error: renderer::gpu_prepare::GpuPreparationError) -> RendererError {
    use renderer::gpu_prepare::GpuPreparationError;

    match error {
        GpuPreparationError::Resource(error) => RendererError::ResidentGpu(error),
        GpuPreparationError::InvalidCamera => RendererError::InvalidCamera,
        GpuPreparationError::InvalidViewport { .. } => RendererError::InvalidConfig,
        GpuPreparationError::Unavailable
        | GpuPreparationError::Raster(_)
        | GpuPreparationError::OutOfMemory(_)
        | GpuPreparationError::Validation(_)
        | GpuPreparationError::Internal(_)
        | GpuPreparationError::ExactContractMismatch { .. }
        | GpuPreparationError::StaleRuntimeGeneration
        | GpuPreparationError::ExecutionOwnerMismatch
        | GpuPreparationError::ExecutionOwnerAlreadyBound
        | GpuPreparationError::PlanSetGenerationRegression { .. }
        | GpuPreparationError::CpuOrderCapacityExceeded { .. }
        | GpuPreparationError::CpuOrderSourceIdOutOfRange { .. } => {
            RendererError::GpuDeviceCreation
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn map_frame_execution_error(error: renderer::FrameExecutionError) -> RendererError {
    match error {
        renderer::FrameExecutionError::GpuPreparation(error) => map_gpu_preparation_error(error),
        renderer::FrameExecutionError::Generation(_)
        | renderer::FrameExecutionError::PlanSet(_)
        | renderer::FrameExecutionError::Raster(_)
        | renderer::FrameExecutionError::ProjectedWorkMismatch { .. }
        | renderer::FrameExecutionError::EncodeAttemptExhausted
        | renderer::FrameExecutionError::PendingFrameMismatch { .. }
        | renderer::FrameExecutionError::Controller(_)
        | renderer::FrameExecutionError::Sampler(_) => RendererError::GpuDeviceCreation,
    }
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
    #[error("geometry path {path:?} needs source data that is unavailable after Packed GPU upload")]
    GeometrySourceUnavailable { path: GeometryPath },
    #[error("surface acquire failed: {0}")]
    SurfaceAcquire(String),
    #[error("surface out of memory")]
    SurfaceOutOfMemory,
    #[error(
        "surface dimensions {width}x{height} exceed the device 2D texture limit {max_dimension}"
    )]
    GpuDimensionsUnsupported {
        width: u32,
        height: u32,
        max_dimension: u32,
    },
    #[error("direct scene resource error: {0}")]
    DirectScene(#[from] DirectSceneError),
    #[error("resident scene encoding error: {0}")]
    ResidentScene(#[from] ResidentSceneError),
    #[error("resident GPU resource error: {0}")]
    ResidentGpu(#[from] ResidentGpuError),
    #[error("resident exact tiled raster error: {0}")]
    ResidentTiled(#[from] ResidentTiledError),
    #[error("paged atlas error: {0}")]
    PagedAtlas(String),
    #[error("paged active atlas is not yet supported on surface presenters")]
    PagedAtlasUnsupported,
    #[error("gpu ordering is only available for the direct geometry path")]
    GpuOrderUnsupported,
    #[error("exact projected contributor compaction is unavailable on this surface adapter")]
    ProjectedCompactionUnsupported,
    #[error(
        "the preproject GPU producer requires PackedAtlas, ProjectedQuadsExact, and forced Compact projected drawing"
    )]
    PreprojectProducerIncompatible,
    #[error("the preproject GPU producer has no valid refreshed order prefix")]
    PreprojectOrderUnavailable,
    #[error("browser preproject GPU producer must be prepared asynchronously before selection")]
    GpuProducerPreparationRequired,
    #[error("browser GPU ordering must be prepared asynchronously before selection")]
    GpuOrderPreparationRequired,
    #[error("browser geometry-path changes must be awaited through setGeometryPathAsync")]
    SurfaceGeometryPreparationRequired,
    #[error(
        "transactional browser geometry switching supports only Direct and Packed; Paged remains a constructor-time diagnostic"
    )]
    SurfaceGeometrySwitchUnsupported,
    #[error("surface geometry {path:?} allocation ran out of GPU memory: {message}")]
    SurfaceGeometryOutOfMemory { path: GeometryPath, message: String },
    #[error("surface geometry {path:?} creation failed validation: {message}")]
    SurfaceGeometryValidation { path: GeometryPath, message: String },
    #[error("surface geometry {path:?} creation failed internally: {message}")]
    SurfaceGeometryInternal { path: GeometryPath, message: String },
    #[error("browser surface resize must be awaited through the transactional resize API")]
    SurfaceResizePreparationRequired,
    #[error(
        "transactional browser resize supports only PackedAtlas with ProjectedQuadsExact raster"
    )]
    SurfaceResizeUnsupported,
    #[error(
        "surface resize failed and restoring the previous configuration also failed: resize={resize_error}; rollback={rollback_error}"
    )]
    SurfaceResizeRollbackFailed {
        resize_error: String,
        rollback_error: String,
    },
    #[error("surface framebuffer capture is unsupported: {0}")]
    SurfaceCaptureUnsupported(String),
    #[error("surface framebuffer capture state is invalid: {0}")]
    SurfaceCaptureState(String),
    #[error("surface framebuffer capture readback failed")]
    SurfaceCaptureReadback,
}

impl SurfacePresenterError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidSurfaceSize
            | Self::ResidentScene(ResidentSceneError::CountMismatch { .. })
            | Self::ResidentScene(ResidentSceneError::InvalidSplat { .. }) => {
                ErrorCode::InvalidArgument
            }
            Self::ResidentGpu(ResidentGpuError::GpuOrderInternal(_)) => ErrorCode::Internal,
            Self::ResidentTiled(ResidentTiledError::Internal(_))
            | Self::ResidentTiled(ResidentTiledError::Validation(_))
            | Self::ResidentTiled(ResidentTiledError::Readback)
            | Self::ResidentTiled(ResidentTiledError::IncompleteScatter)
            | Self::ResidentTiled(ResidentTiledError::OutOfMemory) => ErrorCode::Internal,
            Self::SurfaceCreation
            | Self::NoAdapter
            | Self::DeviceCreation(_)
            | Self::GeometrySourceUnavailable { .. }
            | Self::PagedAtlasUnsupported
            | Self::GpuOrderUnsupported
            | Self::ProjectedCompactionUnsupported
            | Self::PreprojectProducerIncompatible
            | Self::PreprojectOrderUnavailable
            | Self::GpuProducerPreparationRequired
            | Self::GpuOrderPreparationRequired
            | Self::SurfaceGeometryPreparationRequired
            | Self::SurfaceGeometrySwitchUnsupported
            | Self::SurfaceResizePreparationRequired
            | Self::SurfaceResizeUnsupported
            | Self::SurfaceCaptureUnsupported(_) => ErrorCode::Unsupported,
            Self::GpuDimensionsUnsupported { .. } => ErrorCode::Unsupported,
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::NoSurfaceFormat
            | Self::SurfaceConfigure(_)
            | Self::SurfaceAcquire(_)
            | Self::SurfaceOutOfMemory
            | Self::SurfaceGeometryOutOfMemory { .. }
            | Self::SurfaceGeometryValidation { .. }
            | Self::SurfaceGeometryInternal { .. }
            | Self::SurfaceResizeRollbackFailed { .. }
            | Self::SurfaceCaptureState(_)
            | Self::SurfaceCaptureReadback
            | Self::PagedAtlas(_) => ErrorCode::Internal,
            Self::DirectScene(DirectSceneError::ResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::PackedResourceLimitExceeded(_))
            | Self::DirectScene(DirectSceneError::PagedAtlasResourceLimitExceeded { .. })
            | Self::DirectScene(DirectSceneError::ResourceSizeOverflow)
            | Self::ResidentScene(ResidentSceneError::UnsupportedShDegree(_))
            | Self::ResidentScene(ResidentSceneError::SizeOverflow)
            | Self::ResidentScene(ResidentSceneError::UploadStagingReleased)
            | Self::ResidentGpu(_)
            | Self::ResidentTiled(_) => ErrorCode::Unsupported,
            Self::DirectScene(DirectSceneError::SortedIndexCapacityExceeded)
            | Self::DirectScene(DirectSceneError::GpuOrderInitialization(_))
            | Self::ResidentScene(ResidentSceneError::InvalidScene)
            | Self::ResidentScene(ResidentSceneError::AllocationFailed { .. }) => {
                ErrorCode::Internal
            }
        }
    }
}

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    geometry_path: GeometryPath,
    cpu_order_engine: CpuOrderEngine,
    #[cfg(not(target_arch = "wasm32"))]
    gpu_rasterizer: Option<GpuRasterizer>,
    /// Wide source buffers are retained only by the Direct reference and the
    /// experimental paged path. Production Packed loading consumes them.
    scene: Option<SceneBuffers>,
    /// Exact-count compact resident representation used by PackedAtlas.
    resident_scene_cpu: Option<ResidentSceneCpu>,
    /// Sole complete Exact runtime for native Packed rendering. Offscreen and
    /// Surface hosts supply different targets but never own a second scene,
    /// PlanSet, controller, generation ledger, sampler, or raster graph.
    #[cfg(not(target_arch = "wasm32"))]
    exact_offscreen_runtime: Option<renderer::PreparedRuntimeSlot>,
    /// Spatial page metadata for [`GeometryPath::PagedActiveAtlas`].
    spatial_pages: Option<SpatialPageSet>,
    world_covariances: Option<Vec<[[f32; 3]; 3]>>,
    world_covariance_terms: Option<Vec<CameraCovarianceTerms>>,
    alpha_values: Option<Vec<f32>>,
    preprocess_indices: Vec<u32>,
    last_stats: FrameStats,
}

/// Unpublished CPU-side state for a Surface geometry-path transition.
///
/// Runtime WebGPU allocation can complete only after an asynchronous error-
/// scope pop. Keeping the target's derived CPU data here means the live
/// renderer remains wholly on its previous path while that wait is pending.
#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub(crate) struct PreparedRendererGeometryPath {
    path: GeometryPath,
    world_covariances: Option<Vec<[[f32; 3]; 3]>>,
    world_covariance_terms: Option<Vec<CameraCovarianceTerms>>,
    alpha_values: Option<Vec<f32>>,
    spatial_pages: Option<SpatialPageSet>,
}

impl PreparedRendererGeometryPath {
    pub(crate) const fn path(&self) -> GeometryPath {
        self.path
    }

    pub(crate) fn world_covariance_terms(&self) -> Option<&[CameraCovarianceTerms]> {
        self.world_covariance_terms.as_deref()
    }

    pub(crate) fn alpha_values(&self) -> Option<&[f32]> {
        self.alpha_values.as_deref()
    }

    pub(crate) fn spatial_pages(&self) -> Option<&SpatialPageSet> {
        self.spatial_pages.as_ref()
    }
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
            let gpu_rasterizer = GpuRasterizer::create(&config)?;
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
            cpu_order_engine: CpuOrderEngine::default(),
            #[cfg(not(target_arch = "wasm32"))]
            gpu_rasterizer: None,
            scene: None,
            resident_scene_cpu: None,
            #[cfg(not(target_arch = "wasm32"))]
            exact_offscreen_runtime: None,
            spatial_pages: None,
            world_covariances: None,
            world_covariance_terms: None,
            alpha_values: None,
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.gpu_rasterizer.as_ref().map(|gpu| gpu.device.as_ref())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.gpu_rasterizer.as_ref().map(|gpu| gpu.queue.as_ref())
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

    /// Derives all CPU data needed by `path` without changing live renderer
    /// state. The returned value is paired with an unpublished Surface GPU
    /// candidate and committed only after WebGPU error scopes report success.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn prepare_geometry_path_candidate(
        &self,
        path: GeometryPath,
    ) -> Result<Option<PreparedRendererGeometryPath>, RendererError> {
        if self.geometry_path == path {
            return Ok(None);
        }

        let (world_covariances, world_covariance_terms, alpha_values, spatial_pages) = match path {
            GeometryPath::SortedIndexDirect => {
                let scene = self.scene.as_ref().ok_or_else(|| {
                    if self.has_scene() {
                        RendererError::GeometrySourceUnavailable { path }
                    } else {
                        RendererError::SceneNotLoaded
                    }
                })?;
                let world_covariances = precompute_world_covariances(scene);
                let world_covariance_terms = world_covariances
                    .iter()
                    .copied()
                    .map(CameraCovarianceTerms::from_matrix)
                    .collect();
                (
                    Some(world_covariances),
                    Some(world_covariance_terms),
                    Some(precompute_alpha_values(scene)),
                    None,
                )
            }
            GeometryPath::PackedAtlas => {
                if !self.has_scene() {
                    return Err(RendererError::SceneNotLoaded);
                }
                (None, None, None, None)
            }
            GeometryPath::PagedActiveAtlas => {
                let scene = self.scene.as_ref().ok_or_else(|| {
                    if self.has_scene() {
                        RendererError::GeometrySourceUnavailable { path }
                    } else {
                        RendererError::SceneNotLoaded
                    }
                })?;
                (None, None, None, Some(default_spatial_pages(scene)))
            }
        };

        Ok(Some(PreparedRendererGeometryPath {
            path,
            world_covariances,
            world_covariance_terms,
            alpha_values,
            spatial_pages,
        }))
    }

    /// Publishes a previously prepared CPU geometry candidate. Every field
    /// assignment is infallible; no target-path derivation or allocation is
    /// performed in this commit phase.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn publish_geometry_path_candidate(
        &mut self,
        prepared: PreparedRendererGeometryPath,
    ) {
        debug_assert_ne!(self.geometry_path, prepared.path);
        self.geometry_path = prepared.path;
        self.world_covariances = prepared.world_covariances;
        self.world_covariance_terms = prepared.world_covariance_terms;
        self.alpha_values = prepared.alpha_values;
        self.spatial_pages = prepared.spatial_pages;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
            rasterizer.clear_scene_resources();
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
            gpu_rasterizer.ensure_output_target(width, height)?;
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

    /// Reports whether the loaded scene fits the complete resident Packed path
    /// on this renderer's effective offscreen device limits.
    ///
    /// Like [`Self::current_direct_scene_preflight`], a Surface-only renderer
    /// has no device limits to report and returns
    /// [`RendererError::GpuRasterizerUnavailable`] instead of guessing.
    pub fn current_packed_scene_preflight(&self) -> Result<PackedScenePreflight, RendererError> {
        let scene_len = self.scene_len().ok_or(RendererError::SceneNotLoaded)?;
        let sh_degree = self
            .scene_sh_degree()
            .ok_or(RendererError::SceneNotLoaded)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            packed_scene_preflight_with_limits(scene_len, sh_degree, &rasterizer.device.limits())
                .map_err(RendererError::from)
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = (scene_len, sh_degree);
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
        if self.geometry_path == GeometryPath::PackedAtlas {
            // Encode before mutating self so a failed allocation/validation
            // leaves the previously loaded scene intact.
            let resident = ResidentSceneCpu::encode_owned(scene)?;
            return self.load_resident_scene(resident);
        }
        if !scene
            .scale_xyz
            .iter()
            .copied()
            .all(log_scale_has_finite_nonzero_covariance)
            || !scene
                .rotation_xyzw
                .iter()
                .copied()
                .all(rotation_has_finite_nonzero_norm)
        {
            return Err(RendererError::InvalidScene);
        }

        self.scene = Some(scene);
        self.resident_scene_cpu = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.exact_offscreen_runtime = None;
        }
        self.rebuild_path_specific_cpu_data();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
            rasterizer.clear_scene_resources();
        }
        Ok(())
    }

    /// Transactionally publish an already encoded exact resident scene.
    ///
    /// The input is fully validated before any renderer state is changed. This
    /// entrypoint is intentionally limited to [`GeometryPath::PackedAtlas`];
    /// Direct and Paged require their wide source attributes.
    pub fn load_resident_scene(&mut self, resident: ResidentSceneCpu) -> Result<(), RendererError> {
        resident.validate_complete()?;
        if self.geometry_path != GeometryPath::PackedAtlas {
            return Err(RendererError::InvalidScene);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_ref() {
            let candidate = pollster::block_on(
                renderer::PreparedRuntimeSlot::prepare_complete_gpu_candidate(
                    resident,
                    self.exact_offscreen_runtime.as_ref(),
                    &rasterizer.device,
                    &rasterizer.queue,
                    RENDER_TARGET_FORMAT,
                ),
            )
            .map_err(map_prepared_gpu_runtime_error)?;
            self.publish_exact_offscreen_candidate(candidate);
            return Ok(());
        }

        self.scene = None;
        self.resident_scene_cpu = Some(resident);
        self.rebuild_path_specific_cpu_data();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
            rasterizer.clear_scene_resources();
        }
        Ok(())
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn load_resident_scene_with_exact_test_failure(
        &mut self,
        resident: ResidentSceneCpu,
        failure: renderer::CompleteGpuCandidateTestFailure,
    ) -> Result<(), RendererError> {
        resident.validate_complete()?;
        if self.geometry_path != GeometryPath::PackedAtlas {
            return Err(RendererError::InvalidScene);
        }
        let rasterizer = self
            .gpu_rasterizer
            .as_ref()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        let candidate = pollster::block_on(
            renderer::PreparedRuntimeSlot::prepare_complete_gpu_candidate_with_test_failure(
                resident,
                self.exact_offscreen_runtime.as_ref(),
                &rasterizer.device,
                &rasterizer.queue,
                RENDER_TARGET_FORMAT,
                failure,
            ),
        )
        .map_err(map_prepared_gpu_runtime_error)?;
        self.publish_exact_offscreen_candidate(candidate);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn publish_exact_offscreen_candidate(&mut self, candidate: renderer::PreparedRuntimeSlot) {
        self.scene = None;
        self.resident_scene_cpu = None;
        self.exact_offscreen_runtime = Some(candidate);
        self.preprocess_indices.clear();
        self.rebuild_path_specific_cpu_data();
        self.gpu_rasterizer
            .as_mut()
            .expect("offscreen candidate requires the existing rasterizer")
            .clear_scene_resources();
    }

    /// Prepares the native Surface's complete Exact runtime while borrowing
    /// upload-only compact planes from the still-unpublished renderer source.
    /// Failure leaves the source, generations, stats and fallback untouched.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn prepare_surface_exact_candidate(
        &self,
        device: &std::sync::Arc<wgpu::Device>,
        queue: &std::sync::Arc<wgpu::Queue>,
        target_format: wgpu::TextureFormat,
        indirect_execution_supported: bool,
    ) -> Result<renderer::PreparedRuntimeSlot, RendererError> {
        if self.geometry_path != GeometryPath::PackedAtlas || self.gpu_rasterizer.is_some() {
            return Err(RendererError::InvalidConfig);
        }
        let source = self
            .resident_scene_cpu
            .as_ref()
            .ok_or(RendererError::SceneNotLoaded)?;
        renderer::PreparedRuntimeSlot::prepare_complete_surface_gpu_candidate(
            source,
            device,
            queue,
            target_format,
            indirect_execution_supported,
        )
        .await
        .map_err(map_prepared_gpu_runtime_error)
    }

    /// Publishes a fully prepared Surface candidate only if the renderer still
    /// owns the exact source allocation used during preparation.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_surface_exact_candidate(
        &mut self,
        candidate: renderer::PreparedRuntimeSlot,
    ) -> Result<(), RendererError> {
        if self.geometry_path != GeometryPath::PackedAtlas || self.gpu_rasterizer.is_some() {
            return Err(RendererError::InvalidConfig);
        }
        let source = self
            .resident_scene_cpu
            .as_ref()
            .ok_or(RendererError::SceneNotLoaded)?;
        if !candidate.has_same_surface_source(source) {
            return Err(RendererError::SurfacePresenter(
                SurfacePresenterError::SurfaceConfigure(
                    "prepared Surface Exact candidate no longer matches the resident scene".into(),
                ),
            ));
        }
        self.scene = None;
        self.resident_scene_cpu = None;
        self.exact_offscreen_runtime = Some(candidate);
        self.preprocess_indices.clear();
        self.rebuild_path_specific_cpu_data();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn exact_runtime_mut(
        &mut self,
    ) -> Result<&mut renderer::PreparedRuntimeSlot, RendererError> {
        self.exact_offscreen_runtime
            .as_mut()
            .ok_or(RendererError::SceneNotLoaded)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn exact_surface_policy(&self) -> Option<renderer::ExactPlanPolicy> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(renderer::PreparedRuntimeSlot::active_policy)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_exact_surface_policy(
        &mut self,
        policy: renderer::ExactPlanPolicy,
    ) -> Result<(), RendererError> {
        let runtime = self.exact_runtime_mut()?;
        if let renderer::ExactPlanPolicy::Forced(plan) = policy
            && !runtime.plan_is_eligible(plan)
        {
            return Err(SurfacePresenterError::GpuOrderUnsupported.into());
        }
        runtime.set_active_policy(policy);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn exact_surface_plan_is_eligible(&self, plan: plans::PlanId) -> Option<bool> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(|runtime| runtime.plan_is_eligible(plan))
    }

    /// Invalidates only latency-bound whole-plan learning for the active
    /// native Surface runtime. Current-stats tickets and prepared resources
    /// deliberately remain in the same semantic generation.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn reset_exact_surface_performance_learning(&mut self) {
        if let Some(runtime) = self.exact_offscreen_runtime.as_mut() {
            runtime.reset_surface_performance_learning();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request_exact_surface_cpu_refresh(&mut self) -> Result<(), RendererError> {
        self.exact_runtime_mut()?.request_cpu_order_refresh();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn exact_surface_cpu_refresh_requested(&self) -> Option<bool> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(renderer::PreparedRuntimeSlot::cpu_order_refresh_requested)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn exact_surface_last_plan(&self) -> Option<plans::PlanId> {
        self.exact_offscreen_runtime
            .as_ref()
            .and_then(renderer::PreparedRuntimeSlot::last_published_plan)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn exact_surface_adaptive_state(
        &self,
    ) -> Option<renderer::ExactAdaptivePolicyState> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(renderer::PreparedRuntimeSlot::adaptive_policy_state)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request_exact_surface_current_stats(
        &mut self,
    ) -> Result<renderer::CurrentStatsRequest, RendererError> {
        Ok(self.exact_runtime_mut()?.request_current_stats())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn poll_exact_surface_current_stats(
        &mut self,
    ) -> Result<renderer::CurrentStatsPoll, RendererError> {
        Ok(self.exact_runtime_mut()?.poll_current_stats())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_exact_surface_stats(&mut self, stats: FrameStats) {
        debug_assert!(self.exact_offscreen_runtime.is_some());
        self.last_stats = stats;
    }

    /// Commits the CPU side of a successful Surface GPU-resource handoff.
    ///
    /// The presenter must already own complete GPU resources for `uploaded_path`.
    /// Packed staging is validated before it is dropped. Direct/Paged require
    /// the original wide source, while Packed requires an uploadable resident
    /// source; unavailable transitions fail without mutating renderer state.
    pub(crate) fn finish_surface_upload_handoff(
        &mut self,
        uploaded_path: GeometryPath,
    ) -> Result<u64, RendererError> {
        if uploaded_path != self.geometry_path {
            return Err(RendererError::InvalidConfig);
        }
        match uploaded_path {
            GeometryPath::SortedIndexDirect | GeometryPath::PagedActiveAtlas => {
                if self.scene.is_none() {
                    return Err(RendererError::GeometrySourceUnavailable {
                        path: uploaded_path,
                    });
                }
                Ok(0)
            }
            GeometryPath::PackedAtlas => {
                let resident = self.resident_scene_cpu.as_mut().ok_or(
                    RendererError::GeometrySourceUnavailable {
                        path: GeometryPath::PackedAtlas,
                    },
                )?;
                resident.release_upload_staging().map_err(Into::into)
            }
        }
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
                self.spatial_pages = None;
            }
            (GeometryPath::PagedActiveAtlas, Some(scene)) => {
                self.world_covariances = None;
                self.world_covariance_terms = None;
                self.alpha_values = None;
                self.spatial_pages = Some(default_spatial_pages(scene));
            }
            _ => {
                self.world_covariances = None;
                self.world_covariance_terms = None;
                self.alpha_values = None;
                self.spatial_pages = None;
            }
        }
    }

    pub fn scene(&self) -> Option<&SceneBuffers> {
        self.scene.as_ref()
    }

    /// Returns the compact source retained by the production Packed path.
    pub fn resident_scene(&self) -> Option<&ResidentSceneCpu> {
        self.resident_scene_cpu.as_ref().or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.exact_offscreen_runtime
                    .as_ref()
                    .map(|slot| slot.scene().resident())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        })
    }

    /// True for either a wide Direct/Paged source or a compact Packed source.
    pub fn has_scene(&self) -> bool {
        self.scene.is_some() || self.resident_scene().is_some()
    }

    pub fn scene_len(&self) -> Option<usize> {
        self.scene
            .as_ref()
            .map(SceneBuffers::len)
            .or_else(|| self.resident_scene().map(ResidentSceneCpu::len))
    }

    pub fn scene_sh_degree(&self) -> Option<u8> {
        self.scene
            .as_ref()
            .map(|scene| scene.sh_degree)
            .or_else(|| self.resident_scene().map(|scene| scene.sh_degree))
    }

    /// Exact source-order world positions shared by CPU ordering, camera
    /// framing, and benchmark traces for every geometry path.
    pub fn positions(&self) -> Option<&[Vec3f]> {
        self.scene
            .as_ref()
            .map(|scene| scene.positions.as_slice())
            .or_else(|| self.resident_scene().map(|scene| scene.positions.as_ref()))
    }

    pub fn world_covariances(&self) -> Option<&[[[f32; 3]; 3]]> {
        self.world_covariances.as_deref()
    }

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let positions = self.positions().ok_or(RendererError::SceneNotLoaded)?;
        let mut output = PreprocessOutput {
            depth_keys: Vec::with_capacity(positions.len()),
            indices: Vec::with_capacity(positions.len()),
        };
        preprocess_positions_visible_into(
            positions,
            camera,
            &mut output.depth_keys,
            &mut output.indices,
        )?;
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

    fn preprocess_and_sort_timed(&mut self, camera: &Camera) -> Result<(f32, f32), RendererError> {
        let stable_full32 = self.mode == RenderMode::SortedAlpha;
        let timings = match self.geometry_path {
            GeometryPath::PagedActiveAtlas => {
                let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
                let pages = self
                    .spatial_pages
                    .as_ref()
                    .ok_or(RendererError::InvalidScene)?;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let rasterizer = self
                        .gpu_rasterizer
                        .as_mut()
                        .ok_or(RendererError::GpuRasterizerUnavailable)?;
                    rasterizer.ensure_paged_active_set(scene, pages, camera)?;
                    let entries = rasterizer
                        .paged_active_set
                        .as_ref()
                        .ok_or(RendererError::InvalidScene)?
                        .atlas
                        .active_entries();
                    self.cpu_order_engine.order_paged(
                        scene,
                        &entries,
                        camera,
                        stable_full32,
                        &mut self.preprocess_indices,
                    )?
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (scene, pages, camera, stable_full32);
                    return Err(RendererError::GpuRasterizerUnavailable);
                }
            }
            GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => {
                let positions = if let Some(scene) = self.scene.as_ref() {
                    scene.positions.as_slice()
                } else if let Some(scene) = self.resident_scene_cpu.as_ref() {
                    scene.positions.as_ref()
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.exact_offscreen_runtime
                            .as_ref()
                            .map(|slot| slot.scene().positions())
                            .ok_or(RendererError::SceneNotLoaded)?
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        return Err(RendererError::SceneNotLoaded);
                    }
                };
                self.cpu_order_engine.order_positions(
                    CpuPositionView::new(positions),
                    camera,
                    stable_full32,
                    &mut self.preprocess_indices,
                )?
            }
        };
        Ok((timings.preprocess_ms, timings.sort_ms))
    }

    fn record_stats(
        &mut self,
        frame_start: TimerInstant,
        preprocess_ms: f32,
        sort_ms: f32,
        raster_ms: f32,
        drawn_count: u32,
    ) -> FrameStats {
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: self.preprocess_indices.len() as u32,
            drawn_count,
        };
        self.last_stats = stats;
        stats
    }

    pub fn build_sorted_instances_into(
        &mut self,
        camera: &Camera,
        instances: &mut Vec<GpuInstance>,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

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

        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, raster_ms, drawn_count))
    }

    pub fn build_surface_sorted_indices_with_sort_refresh(
        &mut self,
        camera: &Camera,
        refresh_sort: bool,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let refresh_sort = refresh_sort || self.preprocess_indices.is_empty();
        let (preprocess_ms, sort_ms) = if refresh_sort {
            self.preprocess_and_sort_timed(camera)?
        } else {
            camera
                .validate()
                .map_err(|_| RendererError::InvalidCamera)?;
            (0.0, 0.0)
        };

        let drawn_count = self.preprocess_indices.len() as u32;
        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, 0.0, drawn_count))
    }

    pub fn current_sorted_indices(&self) -> &[u32] {
        #[cfg(not(target_arch = "wasm32"))]
        if self.geometry_path == GeometryPath::PackedAtlas
            && let Some(order) = self
                .exact_offscreen_runtime
                .as_ref()
                .and_then(renderer::PreparedRuntimeSlot::last_usable_cpu_order)
        {
            return order;
        }
        &self.preprocess_indices
    }

    pub fn replace_surface_sorted_indices(
        &mut self,
        mut indices: Vec<u32>,
    ) -> Result<(), RendererError> {
        self.replace_surface_sorted_indices_recycling(&mut indices)
    }

    pub(crate) fn replace_surface_sorted_indices_recycling(
        &mut self,
        indices: &mut Vec<u32>,
    ) -> Result<(), RendererError> {
        let scene_len = self.scene_len().ok_or(RendererError::SceneNotLoaded)?;
        match self.geometry_path {
            GeometryPath::PagedActiveAtlas => {
                let max_index = self
                    .spatial_pages
                    .as_ref()
                    .map(|pages| {
                        pages
                            .page_count()
                            .saturating_mul(pages.page_capacity)
                            .saturating_sub(1)
                    })
                    .unwrap_or(0) as u32;
                if indices.iter().any(|&idx| idx > max_index) {
                    return Err(RendererError::InvalidScene);
                }
            }
            GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => {
                if indices.iter().any(|&idx| idx as usize >= scene_len) {
                    return Err(RendererError::InvalidScene);
                }
            }
        }

        std::mem::swap(&mut self.preprocess_indices, indices);
        Ok(())
    }

    pub fn build_sorted_indices(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<u32>, FrameStats), RendererError> {
        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let drawn_count = self.preprocess_indices.len() as u32;
        let stats = self.record_stats(frame_start, preprocess_ms, sort_ms, 0.0, drawn_count);
        Ok((self.preprocess_indices.clone(), stats))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn raster_sorted_indices(
        &mut self,
        camera: &Camera,
        sorted_indices: &[u32],
    ) -> Result<(), RendererError> {
        match self.geometry_path {
            GeometryPath::SortedIndexDirect => self
                .gpu_rasterizer
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?
                .render_direct_sorted_indices(
                    self.config,
                    sorted_indices,
                    camera,
                    self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?,
                    self.world_covariance_terms
                        .as_deref()
                        .ok_or(RendererError::InvalidScene)?,
                    self.alpha_values
                        .as_deref()
                        .ok_or(RendererError::InvalidScene)?,
                ),
            GeometryPath::PackedAtlas => self
                .gpu_rasterizer
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?
                .render_packed_sorted_indices(
                    self.config,
                    sorted_indices,
                    camera,
                    self.resident_scene_cpu
                        .as_ref()
                        .ok_or(RendererError::SceneNotLoaded)?,
                ),
            GeometryPath::PagedActiveAtlas => self
                .gpu_rasterizer
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?
                .render_paged_sorted_indices(
                    self.config,
                    sorted_indices,
                    camera,
                    self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?,
                ),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();
        if self.gpu_rasterizer.is_none() {
            return Err(RendererError::GpuRasterizerUnavailable);
        }

        if self.geometry_path == GeometryPath::PackedAtlas && self.exact_offscreen_runtime.is_some()
        {
            return self.render_packed_exact_offscreen(camera, frame_start);
        }

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let raster_start = timer_now();
        let sorted_indices = std::mem::take(&mut self.preprocess_indices);
        let raster_result = self.raster_sorted_indices(camera, &sorted_indices);
        let drawn_count = sorted_indices.len() as u32;
        self.preprocess_indices = sorted_indices;
        raster_result?;
        let raster_ms = timer_elapsed_ms(raster_start);

        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, raster_ms, drawn_count))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_packed_exact_offscreen(
        &mut self,
        camera: &Camera,
        frame_start: TimerInstant,
    ) -> Result<FrameStats, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        let viewport = renderer::frame::Viewport::new(self.config.width, self.config.height)
            .map_err(|_| RendererError::InvalidConfig)?;
        let rasterizer = self
            .gpu_rasterizer
            .as_ref()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        let slot = self
            .exact_offscreen_runtime
            .as_mut()
            .ok_or(RendererError::SceneNotLoaded)?;
        let request = renderer::GpuFrameEncodeRequest::new(
            plans::PlanId::CpuPostSort,
            camera,
            viewport,
            rasterizer.offscreen_target.view(),
            RENDER_TARGET_FORMAT,
            wgpu::Color::TRANSPARENT,
        )
        .with_forced_cpu_order_refresh()
        .with_host_frame_started(frame_start);
        let pending =
            renderer::encode_frame_gpu(slot, request).map_err(map_frame_execution_error)?;
        let submission =
            renderer::submit_encoded_frame(slot, pending).map_err(map_frame_execution_error)?;
        let timings = submission
            .host_timings()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let visible_count = submission
            .visible_count()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let drawn_count = submission
            .draw_count()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let stats = FrameStats {
            frame_ms: timings.frame_ms(),
            preprocess_ms: timings.preprocess_ms(),
            sort_ms: timings.sort_ms(),
            raster_ms: timings.raster_ms(),
            visible_count,
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
        self.raster_sorted_indices(camera, sorted_indices)?;
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
}

fn make_surface_source_elems(splats: SplatSetView<'_>) -> Vec<GpuSurfaceSourceElem> {
    if splats.is_empty() {
        return vec![GpuSurfaceSourceElem::zeroed()];
    }

    (0..splats.len())
        .map(|i| {
            let position = splats.positions()[i];
            let color_dc = splats.color_dc().get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
            let cov =
                splats
                    .world_covariance_terms()
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
            let alpha = splats.alpha_values().get(i).copied().unwrap_or(0.0);
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
        order_stride_words: 1,
        order_id_offset_words: 0,
        source_position_stride_words: 16,
        source_position_offset_words: 0,
    }
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

    let alpha = unsafe { *alpha_values.get_unchecked(i) }.min(0.99);
    let dir_world = normalize3(Vec3f::new(
        pos_world.x - camera.pose.position.x,
        pos_world.y - camera.pose.position.y,
        pos_world.z - camera.pose.position.z,
    ));
    let rgb = unsafe { sh_color_unchecked(scene, i, dir_world, sh_layout) };

    Some(GpuInstance {
        center_and_axis_u: [x_ndc, y_ndc, axis_u[0], axis_u[1]],
        axis_v_and_pad: [axis_v[0], axis_v[1], 0.0, 0.0],
        color_rgba: [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha],
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
        // The reference 3DGS rasterizer adds 0.3 pixel^2 to covariance
        // (standard deviation sqrt(0.3) px), not a 0.3 px standard deviation.
        let blur_variance_pixels = 0.3_f32;
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
            blur_cov_x: blur_variance_pixels * px_ndc_x.powi(2),
            blur_cov_y: blur_variance_pixels * px_ndc_y.powi(2),
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

fn default_spatial_pages(scene: &SceneBuffers) -> SpatialPageSet {
    let page_capacity = (scene.len() / 4).clamp(1, DEFAULT_PAGE_CAPACITY);
    let grid_axis = ((scene.len() as f32).cbrt().ceil() as usize).clamp(1, 8);
    spatial_pages::partition_scene_pages_with_coarse_cover(
        scene,
        page_capacity,
        grid_axis,
        DEFAULT_PAGED_ATLAS_SLOTS,
    )
}

fn canonical_dot3_f32(left: [f32; 3], right: [f32; 3]) -> f32 {
    // This exact operation order is mirrored by WGSL key generation and
    // projection. Explicit FMA avoids backend-dependent native-dot contraction
    // while retaining the more accurate result for cancellation-heavy inputs.
    left[2].mul_add(right[2], left[1].mul_add(right[1], left[0] * right[0]))
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

    let p = [p.x, p.y, p.z];
    Vec3f::new(
        canonical_dot3_f32(view_rot[0], p),
        canonical_dot3_f32(view_rot[1], p),
        canonical_dot3_f32(view_rot[2], p),
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

    // 3-sigma axes. The draw shader extends the quad to the alpha threshold;
    // retaining the unbounded finite axes is part of the source covariance
    // quality contract.
    let radius_k = 3.0_f32;
    let major_radius = (major.sqrt() * radius_k).max(1e-4);
    let minor_radius = (minor.sqrt() * radius_k).max(1e-4);
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
        out.push(world_covariance_from_source(
            scene.scale_xyz[i],
            scene.rotation_xyzw[i],
        ));
    }
    out
}

/// Produces the canonical covariance used by both the wide Direct oracle and
/// the exact-count resident encoder. Keeping this arithmetic in one function
/// makes the two draw paths bit-identical before color compression.
pub(crate) fn world_covariance_from_source(
    log_scale: [f32; 3],
    rotation_xyzw: [f32; 4],
) -> [[f32; 3]; 3] {
    let sx = log_scale[0].exp();
    let sy = log_scale[1].exp();
    let sz = log_scale[2].exp();
    let object_cov = [
        [sx * sx, 0.0, 0.0],
        [0.0, sy * sy, 0.0],
        [0.0, 0.0, sz * sz],
    ];
    // Preserve the Direct oracle's established normalization sequence.
    let rot_gaussian = quat_to_mat3(quat_normalize(rotation_xyzw));
    mat3_mul(
        mat3_mul(rot_gaussian, object_cov),
        mat3_transpose(rot_gaussian),
    )
}

pub(crate) fn log_scale_has_finite_nonzero_covariance(log_scale: [f32; 3]) -> bool {
    log_scale.into_iter().all(|value| {
        let scale = value.exp();
        scale.is_finite() && scale > 0.0 && (scale * scale).is_finite()
    })
}

pub(crate) fn rotation_has_finite_nonzero_norm(rotation_xyzw: [f32; 4]) -> bool {
    let norm2 = rotation_xyzw
        .into_iter()
        .map(|value| value * value)
        .sum::<f32>();
    norm2.is_finite() && norm2 > 0.0
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

fn packed_color_word(
    scene: &SceneBuffers,
    index: usize,
    camera_position: [f32; 3],
    layout: ShColorLayout<'_>,
) -> u32 {
    let position = scene.positions[index];
    let dir = normalize_dir(
        position.x - camera_position[0],
        position.y - camera_position[1],
        position.z - camera_position[2],
    );
    let rgb = unsafe { sh_color_unchecked(scene, index, dir, layout) };
    pack_color_rgb10([
        rgb[0].clamp(0.0, 1.0),
        rgb[1].clamp(0.0, 1.0),
        rgb[2].clamp(0.0, 1.0),
    ])
}

fn refresh_paged_hot_colors(
    queue: &wgpu::Queue,
    paged: &mut paged_gpu::PagedAtlasGpu,
    scene: &SceneBuffers,
    camera: &Camera,
) {
    let entries = paged.active_entries();
    if entries.is_empty() {
        return;
    }
    let layout = ShColorLayout::new(scene);
    let cam = [
        camera.pose.position.x,
        camera.pose.position.y,
        camera.pose.position.z,
    ];
    let mut run_start = entries[0].0 as usize;
    let mut expected_global = entries[0].0;
    let mut colors = Vec::new();

    for (global_index, scene_index) in entries {
        if global_index != expected_global {
            paged
                .resources
                .write_hot_colors_at(queue, run_start, &colors);
            colors.clear();
            run_start = global_index as usize;
        }
        let index = scene_index as usize;
        colors.push(packed_color_word(scene, index, cam, layout));
        expected_global = global_index.saturating_add(1);
    }
    paged
        .resources
        .write_hot_colors_at(queue, run_start, &colors);
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

struct DirectSceneResources {
    sorted_indices_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    cpu_bind_group: wgpu::BindGroup,
    capacity: usize,
    count: usize,
    sh_degree: u32,
    source_buffer: wgpu::Buffer,
    sh_rest_buffer: wgpu::Buffer,
    gpu_order: Option<DirectGpuSceneOrder>,
}

pub(crate) struct DirectGpuSceneOrder {
    sorter: direct_gpu_order::DirectGpuOrder,
    bind_group: wgpu::BindGroup,
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
        let source_elems = make_surface_source_elems(SplatSetView::new(
            &scene.positions,
            &scene.color_dc,
            world_covariance_terms,
            alpha_values,
        ));
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
        let cpu_bind_group = create_direct_bind_group(
            device,
            bind_group_layout,
            "gsplat-direct-cpu-order-bind-group",
            &sorted_indices_buffer,
            &source_buffer,
            &sh_rest_buffer,
            &params_buffer,
        );

        Ok(Self {
            sorted_indices_buffer,
            params_buffer,
            cpu_bind_group,
            capacity,
            count: scene.len(),
            sh_degree: scene.sh_degree as u32,
            source_buffer,
            sh_rest_buffer,
            gpu_order: None,
        })
    }

    fn prepare_cpu(
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

    pub(crate) fn create_gpu_order_candidate(
        &self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<DirectGpuSceneOrder, DirectSceneError> {
        let count =
            u32::try_from(self.count).map_err(|_| DirectSceneError::SortedIndexCapacityExceeded)?;
        let capacity = u32::try_from(self.capacity)
            .map_err(|_| DirectSceneError::SortedIndexCapacityExceeded)?;
        direct_gpu_order::DirectGpuOrder::validate_soa_dispatch_limits(device, capacity, count)?;
        let sorter = direct_gpu_order::DirectGpuOrder::new_soa(
            device,
            &self.source_buffer,
            &self.params_buffer,
            capacity,
            count,
        )?;
        let bind_group = create_direct_bind_group(
            device,
            bind_group_layout,
            "gsplat-direct-gpu-order-bind-group",
            sorter.final_ids(),
            &self.source_buffer,
            &self.sh_rest_buffer,
            &self.params_buffer,
        );
        Ok(DirectGpuSceneOrder { sorter, bind_group })
    }

    pub(crate) fn publish_gpu_order(&mut self, prepared: DirectGpuSceneOrder) {
        debug_assert!(self.gpu_order.is_none());
        self.gpu_order = Some(prepared);
    }

    fn ensure_gpu_order(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), DirectSceneError> {
        if self.gpu_order.is_some() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (device, bind_group_layout);
            Err(DirectSceneError::GpuOrderInitialization(
                "browser GPU ordering must be prepared asynchronously before selection".into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (validation_scope, oom_scope, internal_scope) = (
                device.push_error_scope(wgpu::ErrorFilter::Validation),
                device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
                device.push_error_scope(wgpu::ErrorFilter::Internal),
            );
            let prepared = self.create_gpu_order_candidate(device, bind_group_layout);
            let internal_error = pollster::block_on(internal_scope.pop());
            let oom_error = pollster::block_on(oom_scope.pop());
            let validation_error = pollster::block_on(validation_scope.pop());
            let scope_error = oom_error.or(internal_error).or(validation_error);
            if let Some(error) = scope_error {
                return Err(DirectSceneError::GpuOrderInitialization(error.to_string()));
            }
            self.publish_gpu_order(prepared?);
            Ok(())
        }
    }

    fn prepare_gpu(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        queue: &wgpu::Queue,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<u32, DirectSceneError> {
        self.ensure_gpu_order(device, bind_group_layout)?;
        let instance_count =
            u32::try_from(self.count).map_err(|_| DirectSceneError::SortedIndexCapacityExceeded)?;
        let params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    fn gpu_order(&self) -> Option<&DirectGpuSceneOrder> {
        self.gpu_order.as_ref()
    }
}

fn create_direct_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    order_buffer: &wgpu::Buffer,
    source_buffer: &wgpu::Buffer,
    sh_rest_buffer: &wgpu::Buffer,
    params_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: order_buffer.as_entire_binding(),
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
    })
}

fn create_direct_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    raster::create_splat_bind_group_layout(device, "gsplat-direct-bgl", 3)
}

fn create_direct_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    raster::create_splat_pipeline(
        device,
        bind_group_layout,
        format,
        raster::SplatPipeline {
            shader_label: "gsplat-direct-shader",
            shader_source: include_str!("../shaders/splat_surface_direct.wgsl"),
            layout_label: "gsplat-direct-pipeline-layout",
            pipeline_label: "gsplat-direct-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    )
}

#[cfg(not(target_arch = "wasm32"))]
struct OffscreenResidentPipelines {
    draw_pipeline: wgpu::RenderPipeline,
    draw_bind_group_layout: wgpu::BindGroupLayout,
    color_pipeline: wgpu::ComputePipeline,
    color_bind_group_layout: wgpu::BindGroupLayout,
}

#[cfg(not(target_arch = "wasm32"))]
struct GpuRasterizer {
    adapter_info: wgpu::AdapterInfo,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    offscreen_target: offscreen::OffscreenTarget,
    max_texture_dimension_2d: u32,
    direct_pipeline: wgpu::RenderPipeline,
    direct_bind_group_layout: wgpu::BindGroupLayout,
    direct_scene: Option<DirectSceneResources>,
    packed_pipeline: wgpu::RenderPipeline,
    packed_bind_group_layout: wgpu::BindGroupLayout,
    resident_pipelines: Option<OffscreenResidentPipelines>,
    resident_scene: Option<resident_gpu::ResidentGpuResources>,
    paged_active_set: Option<paged_active_set::PagedActiveSet>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GpuRasterizer {
    fn create(config: &RendererConfig) -> Result<Self, RendererError> {
        pollster::block_on(Self::create_async(config))
    }

    async fn create_async(config: &RendererConfig) -> Result<Self, RendererError> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| RendererError::GpuRasterizerUnavailable)?;

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
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;

        let offscreen_target = offscreen::OffscreenTarget::new(
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
        let resident_pipelines = (device.limits().max_storage_buffers_per_shader_stage
            >= resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS)
            .then(|| {
                let draw_bind_group_layout =
                    resident_gpu::create_resident_draw_bind_group_layout(&device);
                let draw_pipeline = resident_gpu::create_resident_draw_pipeline(
                    &device,
                    &draw_bind_group_layout,
                    RENDER_TARGET_FORMAT,
                );
                let color_bind_group_layout =
                    resident_gpu::create_resident_color_bind_group_layout(&device);
                let color_pipeline =
                    resident_gpu::create_resident_color_pipeline(&device, &color_bind_group_layout);
                OffscreenResidentPipelines {
                    draw_pipeline,
                    draw_bind_group_layout,
                    color_pipeline,
                    color_bind_group_layout,
                }
            });

        Ok(Self {
            adapter_info,
            device,
            queue,
            offscreen_target,
            max_texture_dimension_2d,
            direct_pipeline,
            direct_bind_group_layout,
            direct_scene: None,
            packed_pipeline,
            packed_bind_group_layout,
            resident_pipelines,
            resident_scene: None,
            paged_active_set: None,
        })
    }

    fn clear_scene_resources(&mut self) {
        self.direct_scene = None;
        self.resident_scene = None;
        self.paged_active_set = None;
    }

    fn render_direct_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
    ) -> Result<(), RendererError> {
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
            .ok_or(RendererError::GpuDeviceCreation)?;
        let instance_count = direct_scene
            .prepare_cpu(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let commands = raster::encode_splat_draw(
            &self.device,
            "gsplat-offscreen-direct-encoder",
            raster::SplatDraw {
                pass_label: "gsplat-offscreen-direct-pass",
                view: self.output_view(),
                pipeline: &self.direct_pipeline,
                bind_group: &direct_scene.cpu_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: raster::QUAD_VERTEX_COUNT,
                instance_count,
            },
        );
        self.queue.submit(Some(commands));
        Ok(())
    }

    fn render_packed_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &ResidentSceneCpu,
    ) -> Result<(), RendererError> {
        self.ensure_output_target(config.width, config.height)?;
        let resident_pipelines = self.resident_pipelines.as_ref().ok_or_else(|| {
            resident_gpu::ResidentGpuError::StorageBindingCountUnsupported(
                self.device.limits().max_storage_buffers_per_shader_stage,
            )
        })?;
        // Unlike a SurfacePresenter handoff, this offscreen resource is still
        // owned by Renderer and is cleared by the public geometry-path setter.
        // Keep upload staging so Packed -> other path -> Packed can recreate
        // the exact same complete GPU scene; releasing it here would make that
        // existing lifecycle fail after an otherwise successful frame.
        if self.resident_scene.is_none() {
            self.resident_scene = Some(resident_gpu::ResidentGpuResources::new(
                &self.device,
                &resident_pipelines.draw_bind_group_layout,
                &resident_pipelines.color_bind_group_layout,
                scene,
            )?);
        }
        let instance_count = self
            .resident_scene
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?
            .prepare_cpu_order(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-offscreen-resident-encoder"),
            });
        self.resident_scene
            .as_mut()
            .ok_or(RendererError::GpuDeviceCreation)?
            .encode_color_resolve_if_needed(
                &self.queue,
                &resident_pipelines.color_pipeline,
                &mut encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
        let resident = self
            .resident_scene
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?;
        raster::encode_splat_draw_into(
            &mut encoder,
            &raster::SplatDraw {
                pass_label: "gsplat-offscreen-resident-pass",
                view: self.output_view(),
                pipeline: &resident_pipelines.draw_pipeline,
                bind_group: &resident.draw_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: raster::QUAD_VERTEX_COUNT,
                instance_count,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn ensure_paged_active_set(
        &mut self,
        scene: &SceneBuffers,
        pages: &SpatialPageSet,
        camera: &Camera,
    ) -> Result<(), RendererError> {
        if self.paged_active_set.is_none() {
            self.paged_active_set = Some(paged_active_set::PagedActiveSet::new(
                &self.device,
                &self.packed_bind_group_layout,
                scene,
                pages.clone(),
            )?);
        }

        self.paged_active_set
            .as_mut()
            .ok_or(RendererError::InvalidScene)?
            .sync(&self.queue, scene, camera)
    }

    fn render_paged_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
    ) -> Result<(), RendererError> {
        self.ensure_output_target(config.width, config.height)?;
        let paged = self
            .paged_active_set
            .as_mut()
            .ok_or(RendererError::GpuDeviceCreation)?;
        refresh_paged_hot_colors(&self.queue, &mut paged.atlas, scene, camera);
        let instance_count = paged
            .atlas
            .resources
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let paged = self
            .paged_active_set
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let commands = raster::encode_splat_draw(
            &self.device,
            "gsplat-offscreen-paged-encoder",
            raster::SplatDraw {
                pass_label: "gsplat-offscreen-paged-pass",
                view: self.output_view(),
                pipeline: &self.packed_pipeline,
                bind_group: &paged.atlas.resources.bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: raster::QUAD_VERTEX_COUNT,
                instance_count,
            },
        );
        self.queue.submit(Some(commands));
        Ok(())
    }

    fn readback_rgba8(&mut self) -> Result<Vec<u8>, RendererError> {
        offscreen::readback_rgba8(&self.device, &self.queue, &self.offscreen_target)
    }

    fn output_view(&self) -> &wgpu::TextureView {
        self.offscreen_target.view()
    }

    fn ensure_output_target(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        self.offscreen_target.ensure_size(
            &self.device,
            width,
            height,
            self.max_texture_dimension_2d,
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn offscreen_device_limits(
    config: &RendererConfig,
    adapter_limits: &wgpu::Limits,
) -> Result<wgpu::Limits, RendererError> {
    if config.width == 0 || config.height == 0 {
        return Err(RendererError::InvalidConfig);
    }

    let requested_dimension = config.width.max(config.height);
    if requested_dimension > adapter_limits.max_texture_dimension_2d {
        return Err(RendererError::GpuDimensionsUnsupported {
            width: config.width,
            height: config.height,
            max_dimension: adapter_limits.max_texture_dimension_2d,
        });
    }

    let mut required_limits = wgpu::Limits::downlevel_defaults();
    // The offscreen renderer can switch paths and load a scene only after
    // device creation. Preserve the adapter's storage and texture headroom so
    // later Packed preflight describes physical capabilities rather than an
    // artificial constructor-time default limit. Requesting limits does not
    // allocate buffers.
    required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    required_limits.max_storage_buffer_binding_size =
        adapter_limits.max_storage_buffer_binding_size;
    required_limits.max_buffer_size = adapter_limits.max_buffer_size;
    required_limits.max_storage_buffers_per_shader_stage =
        adapter_limits.max_storage_buffers_per_shader_stage;
    if !required_limits.check_limits(adapter_limits) {
        return Err(RendererError::GpuDeviceCreation);
    }
    Ok(required_limits)
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::Arc;

    use crate::spatial_pages::PageId;
    use gsplat_core::{
        Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f,
    };

    #[cfg(not(target_arch = "wasm32"))]
    use super::offscreen_device_limits;
    #[cfg(not(target_arch = "wasm32"))]
    use super::renderer::{CompleteGpuCandidateTestFailure, PreparedRuntimeSlot};
    use super::{
        DirectSceneError, DirectScenePath, DirectSceneResource, GeometryPath, PackedScenePath,
        PackedScenePreflightFailure, Renderer, RendererError, build_instances,
        direct_scene_preflight, ellipse_axes_from_covariance, packed_scene_preflight_with_limits,
        project_covariance_to_ndc, quat_inverse, try_prepare_then_commit,
    };

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

    fn exact_equal_depth_scene(sh_degree: u8, count: usize) -> SceneBuffers {
        let coefficients_per_splat = ((sh_degree as usize + 1).pow(2) - 1) * 3;
        SceneBuffers {
            positions: (0..count)
                .map(|index| {
                    let x = if count <= 1 {
                        0.0
                    } else {
                        index as f32 / (count - 1) as f32 - 0.5
                    };
                    Vec3f::new(x, (index % 3) as f32 * 0.015 - 0.015, 2.0)
                })
                .collect(),
            opacity: vec![2.0; count],
            scale_xyz: vec![[-3.5, -3.5, -3.5]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: (0..count)
                .map(|index| [0.1 + (index % 5) as f32 * 0.03, -0.05, 0.2])
                .collect(),
            sh_degree,
            sh_rest: (coefficients_per_splat != 0).then(|| {
                (0..count * coefficients_per_splat)
                    .map(|index| ((index % 13) as f32 - 6.0) * 0.01)
                    .collect()
            }),
        }
    }

    fn test_config(size: u32) -> RendererConfig {
        RendererConfig {
            width: size,
            height: size,
            mode: RenderMode::SortedAlpha,
        }
    }

    fn test_renderer(config: RendererConfig, label: &str) -> Option<Renderer> {
        match Renderer::with_config(config) {
            Ok(renderer) => Some(renderer),
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping {label}; adapter unavailable");
                None
            }
            Err(error) => panic!("renderer init: {error}"),
        }
    }

    struct RenderPair {
        first_stats: FrameStats,
        second_stats: FrameStats,
        first_rgba: Vec<u8>,
        second_rgba: Vec<u8>,
    }

    fn render_path_pair(
        scene: SceneBuffers,
        config: RendererConfig,
        camera: &Camera,
        first_path: GeometryPath,
        second_path: GeometryPath,
        label: &str,
    ) -> Option<RenderPair> {
        let mut first = test_renderer(config, label)?;
        let mut second = Renderer::with_config(config).expect("second renderer");
        first.set_geometry_path(first_path);
        second.set_geometry_path(second_path);
        first.load_scene(scene.clone()).unwrap();
        second.load_scene(scene).unwrap();
        let first_stats = first.render_frame(camera).unwrap();
        let second_stats = second.render_frame(camera).unwrap();
        Some(RenderPair {
            first_stats,
            second_stats,
            first_rgba: first.readback_rgba8().unwrap(),
            second_rgba: second.readback_rgba8().unwrap(),
        })
    }

    fn render_direct_packed(scene: SceneBuffers, size: u32, label: &str) -> Option<RenderPair> {
        render_path_pair(
            scene,
            test_config(size),
            &Camera::default(),
            GeometryPath::SortedIndexDirect,
            GeometryPath::PackedAtlas,
            label,
        )
    }

    fn render_packed_paged(scene: SceneBuffers, size: u32, label: &str) -> Option<RenderPair> {
        render_path_pair(
            scene,
            test_config(size),
            &Camera::default(),
            GeometryPath::PackedAtlas,
            GeometryPath::PagedActiveAtlas,
            label,
        )
    }

    #[test]
    fn failed_geometry_resource_prepare_does_not_commit_partial_state() {
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct State {
            path: GeometryPath,
            direct: Option<&'static str>,
            packed: Option<&'static str>,
            paged: Option<&'static str>,
        }

        let mut state = State {
            path: GeometryPath::SortedIndexDirect,
            direct: Some("working-direct"),
            packed: None,
            paged: None,
        };
        let original = state.clone();
        let result = try_prepare_then_commit(
            &mut state,
            |_| Err::<(Option<&str>, Option<&str>, Option<&str>), _>("injected allocation failure"),
            |state, (direct, packed, paged)| {
                state.path = GeometryPath::PagedActiveAtlas;
                state.direct = direct;
                state.packed = packed;
                state.paged = paged;
            },
        );

        assert_eq!(result, Err("injected allocation failure"));
        assert_eq!(state, original);
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
        let config = test_config(64);
        let Some(mut renderer) = test_renderer(config, "packed atlas GPU smoke") else {
            return;
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
    fn packed_exact_offscreen_uses_host_gpu_and_refreshes_identical_camera_order() {
        let config = test_config(64);
        let Some(mut renderer) = test_renderer(config, "packed exact owner and forced order")
        else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer.load_scene(exact_equal_depth_scene(3, 37)).unwrap();

        let rasterizer = renderer.gpu_rasterizer.as_ref().expect("offscreen owner");
        let slot = renderer
            .exact_offscreen_runtime
            .as_ref()
            .expect("Packed Exact runtime");
        assert!(slot.same_gpu_arc_owner(&rasterizer.device, &rasterizer.queue));
        assert_eq!(slot.current_cpu_order_generation(), Some(0));

        let first = renderer.render_frame(&Camera::default()).unwrap();
        assert_eq!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .and_then(PreparedRuntimeSlot::current_cpu_order_generation),
            Some(1)
        );
        let second = renderer.render_frame(&Camera::default()).unwrap();
        let slot = renderer.exact_offscreen_runtime.as_ref().unwrap();
        assert_eq!(slot.current_cpu_order_generation(), Some(2));
        assert_eq!(
            slot.last_usable_cpu_order(),
            Some((0_u32..37).collect::<Vec<_>>().as_slice())
        );
        assert_eq!(
            renderer.current_sorted_indices(),
            (0_u32..37).collect::<Vec<_>>().as_slice(),
            "the public compatibility accessor must expose Exact CPU PostSort"
        );

        for stats in [first, second] {
            assert_eq!(stats.visible_count, 37);
            assert_eq!(stats.drawn_count, 37);
            for duration in [
                stats.preprocess_ms,
                stats.sort_ms,
                stats.raster_ms,
                stats.frame_ms,
            ] {
                assert!(duration.is_finite() && duration >= 0.0);
            }
            assert!(stats.frame_ms + f32::EPSILON >= stats.preprocess_ms);
            assert!(stats.frame_ms + f32::EPSILON >= stats.sort_ms);
            assert!(stats.frame_ms + f32::EPSILON >= stats.raster_ms);
        }
    }

    #[test]
    fn packed_exact_load_clears_old_order_until_the_first_new_exact_frame() {
        let Some(mut renderer) = test_renderer(test_config(64), "Packed Exact order replacement")
        else {
            return;
        };
        renderer.load_scene(build_scene()).unwrap();
        renderer.render_frame(&Camera::default()).unwrap();
        assert_eq!(renderer.current_sorted_indices().len(), 2);

        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer.load_scene(exact_equal_depth_scene(2, 5)).unwrap();
        assert!(
            renderer.current_sorted_indices().is_empty(),
            "a new unpublished Exact scene cannot expose the prior scene order"
        );

        let stats = renderer.render_frame(&Camera::default()).unwrap();
        assert_eq!(stats.visible_count, 5);
        assert_eq!(stats.drawn_count, 5);
        assert_eq!(renderer.current_sorted_indices(), &[0, 1, 2, 3, 4]);
    }

    #[test]
    fn packed_exact_offscreen_sh0_through_sh3_match_direct_with_all_equal_depth_ties() {
        let config = test_config(96);
        let Some(mut direct) = test_renderer(config, "Packed Exact SH0-SH3 Direct oracle") else {
            return;
        };
        let mut packed = Renderer::with_config(config).expect("second renderer");
        direct.set_geometry_path(GeometryPath::SortedIndexDirect);
        packed.set_geometry_path(GeometryPath::PackedAtlas);

        for degree in 0..=3 {
            let scene = exact_equal_depth_scene(degree, 37);
            direct.load_scene(scene.clone()).unwrap();
            packed.load_scene(scene).unwrap();
            let direct_stats = direct.render_frame(&Camera::default()).unwrap();
            let packed_stats = packed.render_frame(&Camera::default()).unwrap();
            assert_eq!(direct_stats.visible_count, 37);
            assert_eq!(direct_stats.drawn_count, 37);
            assert_eq!(packed_stats.visible_count, 37);
            assert_eq!(packed_stats.drawn_count, 37);
            assert_eq!(packed.scene_sh_degree(), Some(degree));
            assert_eq!(
                packed
                    .exact_offscreen_runtime
                    .as_ref()
                    .and_then(PreparedRuntimeSlot::last_usable_cpu_order),
                Some((0_u32..37).collect::<Vec<_>>().as_slice())
            );
            assert_image_parity(
                &format!("Packed Exact SH{degree} Direct oracle"),
                &direct.readback_rgba8().unwrap(),
                &packed.readback_rgba8().unwrap(),
            );
        }
    }

    #[test]
    fn direct_and_paged_offscreen_never_instantiate_the_packed_exact_slot() {
        let Some(mut renderer) = test_renderer(test_config(64), "Direct/Paged Exact isolation")
        else {
            return;
        };
        renderer.load_scene(build_scene()).unwrap();
        assert!(renderer.exact_offscreen_runtime.is_none());
        renderer.set_geometry_path(GeometryPath::PagedActiveAtlas);
        renderer.load_scene(build_scene()).unwrap();
        assert!(renderer.exact_offscreen_runtime.is_none());
    }

    #[test]
    fn packed_exact_failed_frame_and_resize_preserve_published_state() {
        let config = test_config(64);
        let Some(mut renderer) = test_renderer(config, "Packed Exact transactional resize") else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();
        let stats = renderer.render_frame(&Camera::default()).unwrap();
        let image = renderer.readback_rgba8().unwrap();
        let frame = renderer
            .exact_offscreen_runtime
            .as_ref()
            .unwrap()
            .frame_state();
        let order_generation = renderer
            .exact_offscreen_runtime
            .as_ref()
            .unwrap()
            .current_cpu_order_generation();

        let mut invalid_camera = Camera::default();
        invalid_camera.pose.position.x = f32::NAN;
        assert!(matches!(
            renderer.render_frame(&invalid_camera),
            Err(RendererError::InvalidCamera)
        ));
        assert_eq!(renderer.last_stats(), stats);
        assert_eq!(renderer.readback_rgba8().unwrap(), image);
        assert_eq!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .unwrap()
                .frame_state(),
            frame
        );
        assert_eq!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .unwrap()
                .current_cpu_order_generation(),
            order_generation
        );

        let max_dimension = renderer
            .gpu_rasterizer
            .as_ref()
            .unwrap()
            .max_texture_dimension_2d;
        let unsupported_width = max_dimension.checked_add(1).expect("finite texture limit");
        assert!(matches!(
            renderer.set_size(unsupported_width, config.height),
            Err(RendererError::GpuDimensionsUnsupported { .. })
        ));
        assert_eq!(renderer.config(), config);
        assert_eq!(renderer.last_stats(), stats);
        assert_eq!(renderer.readback_rgba8().unwrap(), image);
        assert_eq!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .unwrap()
                .frame_state(),
            frame
        );

        // Exercise an actual WebGPU validation scope after candidate texture
        // creation. The live target remains the old 64x64 image.
        let device = Arc::clone(&renderer.gpu_rasterizer.as_ref().unwrap().device);
        let scoped_error = renderer
            .gpu_rasterizer
            .as_mut()
            .unwrap()
            .offscreen_target
            .ensure_size(&device, 0, config.height, max_dimension);
        assert!(matches!(
            scoped_error,
            Err(RendererError::GpuDeviceCreation)
        ));
        assert_eq!(
            renderer
                .gpu_rasterizer
                .as_ref()
                .unwrap()
                .offscreen_target
                .size(),
            (config.width, config.height)
        );
        assert_eq!(renderer.readback_rgba8().unwrap(), image);

        renderer.set_size(80, 48).unwrap();
        assert_eq!(renderer.config().width, 80);
        assert_eq!(renderer.config().height, 48);
        assert_eq!(renderer.last_stats(), stats);
        assert_eq!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .unwrap()
                .frame_state(),
            frame,
            "resize is not a semantic frame publication"
        );
        let resized = renderer.render_frame(&Camera::default()).unwrap();
        assert_eq!(resized.visible_count, 2);
        assert_eq!(resized.drawn_count, 2);
        let first_readback = renderer.readback_rgba8().unwrap();
        let second_readback = renderer.readback_rgba8().unwrap();
        assert_eq!(first_readback.len(), 80 * 48 * 4);
        assert_eq!(first_readback, second_readback);
        assert_eq!(renderer.last_stats(), resized);
        assert_eq!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .unwrap()
                .current_cpu_order_generation(),
            Some(2),
            "readback must not rerun or mutate ordering policy"
        );
    }

    #[test]
    fn packed_exact_failed_replacement_stages_preserve_scene_image_generations_and_stats() {
        let Some(mut renderer) = test_renderer(
            test_config(64),
            "Packed Exact transactional replacement stages",
        ) else {
            return;
        };
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer.load_scene(exact_equal_depth_scene(3, 37)).unwrap();
        let old_stats = renderer.render_frame(&Camera::default()).unwrap();
        let old_image = renderer.readback_rgba8().unwrap();
        let old_positions = renderer.positions().unwrap().to_vec();
        let old_frame = renderer
            .exact_offscreen_runtime
            .as_ref()
            .unwrap()
            .frame_state();
        let old_order = renderer.current_sorted_indices().to_vec();

        let mut invalid = super::ResidentSceneCpu::encode_owned(exact_equal_depth_scene(3, 5))
            .expect("invalid replacement staging");
        invalid.report.encoded_count -= 1;
        assert!(renderer.load_resident_scene(invalid).is_err());

        for failure in [
            CompleteGpuCandidateTestFailure::GpuResource,
            CompleteGpuCandidateTestFailure::Plan,
            CompleteGpuCandidateTestFailure::Raster,
        ] {
            let replacement = super::ResidentSceneCpu::encode_owned(exact_equal_depth_scene(3, 5))
                .expect("replacement resident");
            assert!(
                renderer
                    .load_resident_scene_with_exact_test_failure(replacement, failure)
                    .is_err(),
                "{failure:?} must fail before publication"
            );
            assert_eq!(renderer.positions(), Some(old_positions.as_slice()));
            assert_eq!(renderer.current_sorted_indices(), old_order);
            assert_eq!(renderer.last_stats(), old_stats);
            assert_eq!(renderer.readback_rgba8().unwrap(), old_image);
            assert_eq!(
                renderer
                    .exact_offscreen_runtime
                    .as_ref()
                    .unwrap()
                    .frame_state(),
                old_frame
            );
        }

        renderer
            .load_scene(exact_equal_depth_scene(3, 5))
            .expect("clean retry publishes one complete candidate");
        assert_eq!(renderer.scene_len(), Some(5));
        assert_ne!(
            renderer
                .exact_offscreen_runtime
                .as_ref()
                .unwrap()
                .frame_state(),
            old_frame
        );
        let replacement_stats = renderer.render_frame(&Camera::default()).unwrap();
        assert_eq!(replacement_stats.visible_count, 5);
        assert_eq!(replacement_stats.drawn_count, 5);
    }

    #[test]
    fn packed_vs_direct_count_parity_on_minimal_scene() {
        let Some(pair) = render_direct_packed(build_scene(), 64, "packed-vs-direct parity") else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        assert_eq!(pair.first_stats.visible_count, 2);
    }

    #[test]
    fn paged_diagnostic_reports_small_scene_counts_without_quality_claim() {
        let Some(pair) = render_packed_paged(build_scene(), 64, "paged diagnostic counts") else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        assert_eq!(pair.first_stats.visible_count, 2);
        assert!(pair.first_rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(pair.second_rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn paged_diagnostic_degree3_is_explicit_and_count_observable() {
        let scene = synthetic_degree3_scene();
        let pages = super::default_spatial_pages(&scene);
        assert_eq!(
            pages.page_count(),
            super::DEFAULT_PAGED_ATLAS_SLOTS,
            "qualification-small scene must fill the fixed multi-page budget"
        );
        assert_eq!(pages.total_splats(), scene.len());
        let Some(pair) = render_packed_paged(scene, 128, "paged qualification parity") else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        assert!(
            pair.first_stats.visible_count > 0,
            "qualification-small parity camera must see splats"
        );
        // PagedActiveAtlas is an explicit diagnostic path, not a release
        // fallback and not a quality oracle. Production quality gates compare
        // Direct against exact-count resident Packed instead.
        assert!(pair.first_rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(pair.second_rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn paged_fixed_budget_evicts_and_excludes_nonresident_pages_from_draw() {
        let scene = paged_eviction_scene();
        let pages = super::default_spatial_pages(&scene);
        assert!(pages.page_count() > super::DEFAULT_PAGED_ATLAS_SLOTS);
        let config = test_config(64);
        let Some(mut renderer) = test_renderer(config, "paged fixed-budget gate") else {
            return;
        };
        renderer.set_geometry_path(super::GeometryPath::PagedActiveAtlas);
        renderer.load_scene(scene).unwrap();

        let mut first_camera = Camera::default();
        first_camera.pose.position = Vec3f::new(0.0, 0.0, -30.0);
        let first_stats = renderer.render_frame(&first_camera).unwrap();
        let (first_residents, first_entries) = {
            let rasterizer = renderer.gpu_rasterizer.as_ref().unwrap();
            let active_set = rasterizer.paged_active_set.as_ref().unwrap();
            let atlas = &active_set.atlas;
            let manager = &active_set.residency;
            assert_eq!(atlas.slot_count(), super::DEFAULT_PAGED_ATLAS_SLOTS);
            assert_eq!(
                atlas.occupied_slot_count(),
                super::DEFAULT_PAGED_ATLAS_SLOTS
            );
            assert_eq!(manager.resident_count(), super::DEFAULT_PAGED_ATLAS_SLOTS);
            (manager.resident_page_ids(), atlas.active_entries())
        };
        assert_eq!(first_stats.drawn_count as usize, first_entries.len());
        assert_active_entries_are_resident(&pages, &first_residents, &first_entries);
        assert!(first_entries.len() < pages.total_splats());

        let mut jumped_camera = Camera::default();
        jumped_camera.pose.position = Vec3f::new(0.0, 0.0, 30.0);
        jumped_camera.pose.rotation_xyzw = [0.0, 1.0, 0.0, 0.0];
        let jumped_stats = renderer.render_frame(&jumped_camera).unwrap();
        let (jumped_residents, jumped_entries) = {
            let rasterizer = renderer.gpu_rasterizer.as_ref().unwrap();
            let active_set = rasterizer.paged_active_set.as_ref().unwrap();
            let atlas = &active_set.atlas;
            let manager = &active_set.residency;
            assert_eq!(atlas.slot_count(), super::DEFAULT_PAGED_ATLAS_SLOTS);
            assert_eq!(
                atlas.occupied_slot_count(),
                super::DEFAULT_PAGED_ATLAS_SLOTS
            );
            assert_eq!(manager.resident_count(), super::DEFAULT_PAGED_ATLAS_SLOTS);
            (manager.resident_page_ids(), atlas.active_entries())
        };
        assert_ne!(
            first_residents, jumped_residents,
            "camera jump must evict pages"
        );
        assert_eq!(jumped_stats.drawn_count as usize, jumped_entries.len());
        assert_active_entries_are_resident(&pages, &jumped_residents, &jumped_entries);
        assert!(jumped_entries.len() < pages.total_splats());
    }

    #[test]
    fn paged_small_motion_trace_retains_cover_without_zero_draw_holes() {
        let scene = paged_eviction_scene();
        let config = test_config(64);
        let Some(mut renderer) = test_renderer(config, "paged small-motion trace") else {
            return;
        };
        renderer.set_geometry_path(super::GeometryPath::PagedActiveAtlas);
        renderer.load_scene(scene).unwrap();

        let trace = [
            Vec3f::new(-3.0, -3.0, 0.0),
            Vec3f::new(-2.9, -3.0, 0.0),
            Vec3f::new(-2.8, -2.9, 0.0),
            Vec3f::new(-2.7, -2.8, 0.0),
        ];
        let mut baseline_residents = None;
        for (frame, position) in trace.into_iter().enumerate() {
            let mut camera = Camera::default();
            camera.pose.position = position;
            let stats = renderer.render_frame(&camera).unwrap();
            assert!(stats.drawn_count > 0, "trace frame {frame} must draw");
            assert!(
                renderer
                    .readback_rgba8()
                    .unwrap()
                    .chunks_exact(4)
                    .any(|pixel| pixel[3] > 0),
                "trace frame {frame} must retain visible coverage"
            );
            let mut residents = renderer
                .gpu_rasterizer
                .as_ref()
                .unwrap()
                .paged_active_set
                .as_ref()
                .unwrap()
                .residency
                .resident_page_ids();
            residents.sort_by_key(|page_id| page_id.0);
            if let Some(baseline) = &baseline_residents {
                assert_eq!(
                    &residents, baseline,
                    "small motion must retain the coarse resident cover"
                );
            } else {
                baseline_residents = Some(residents);
            }
        }
    }

    #[test]
    fn surface_paged_local_runtime_prepares_stable_nonzero_draws() {
        let scene = paged_eviction_scene();
        let config = test_config(64);
        let Some(renderer) = test_renderer(config, "Surface paged runtime gate") else {
            return;
        };
        let device = renderer.device().unwrap().clone();
        let queue = renderer.queue().unwrap().clone();
        let layout = super::packed_gpu::create_packed_bind_group_layout(&device);
        let pages = super::default_spatial_pages(&scene);
        let page_count = pages.page_count();
        let page_capacity = pages.page_capacity;
        assert!(page_count > super::DEFAULT_PAGED_ATLAS_SLOTS);
        let mut runtime = super::SurfacePagedRuntime::new(&device, &layout, &scene, pages).unwrap();
        assert_eq!(
            runtime.active_set.atlas.resources.capacity,
            4 * page_capacity
        );
        assert!(runtime.active_set.atlas.resources.capacity < scene.len());

        for position in [
            Vec3f::new(-3.0, -3.0, 0.0),
            Vec3f::new(-2.9, -3.0, 0.0),
            Vec3f::new(-2.8, -2.9, 0.0),
        ] {
            let mut camera = Camera::default();
            camera.pose.position = position;
            let drawn = runtime
                .prepare(&queue, &scene, &camera, config.width, config.height)
                .unwrap();
            assert!(
                drawn > 0,
                "Surface paged runtime must prepare non-zero draw"
            );
            assert_eq!(
                runtime.active_set.atlas.slot_count(),
                super::DEFAULT_PAGED_ATLAS_SLOTS
            );
            assert_eq!(
                runtime.active_set.atlas.occupied_slot_count(),
                super::DEFAULT_PAGED_ATLAS_SLOTS
            );
        }
    }

    #[test]
    fn paged_bounded_trace_keeps_slot_resident_and_active_counts_fixed() {
        let scene = paged_eviction_scene();
        let config = test_config(64);
        let Some(mut renderer) = test_renderer(config, "paged bounded trace") else {
            return;
        };
        renderer.set_geometry_path(super::GeometryPath::PagedActiveAtlas);
        renderer.load_scene(scene).unwrap();

        for frame in 0..512 {
            let phase = frame as f32 / 511.0 * std::f32::consts::TAU;
            let mut camera = Camera::default();
            camera.pose.position = Vec3f::new(phase.sin() * 3.0, phase.cos() * 3.0, 0.0);
            let stats = renderer.render_frame(&camera).unwrap();
            let rasterizer = renderer.gpu_rasterizer.as_ref().unwrap();
            let active_set = rasterizer.paged_active_set.as_ref().unwrap();
            let atlas = &active_set.atlas;
            let manager = &active_set.residency;
            let active = atlas.active_entries().len();
            assert_eq!(atlas.slot_count(), super::DEFAULT_PAGED_ATLAS_SLOTS);
            assert!(atlas.occupied_slot_count() <= super::DEFAULT_PAGED_ATLAS_SLOTS);
            assert!(manager.resident_count() <= super::DEFAULT_PAGED_ATLAS_SLOTS);
            assert!(active <= atlas.slot_count().saturating_mul(atlas.page_capacity));
            assert_eq!(active, atlas.resident_splat_count());
            assert_eq!(stats.drawn_count as usize, active);
            assert!(
                stats.drawn_count > 0,
                "bounded trace frame {frame} must draw"
            );
        }
    }

    fn assert_active_entries_are_resident(
        pages: &super::SpatialPageSet,
        resident_pages: &[PageId],
        active_entries: &[(u32, u32)],
    ) {
        let resident_scene_indices: std::collections::HashSet<u32> = resident_pages
            .iter()
            .flat_map(|&page_id| pages.page(page_id).unwrap().splat_indices.iter().copied())
            .collect();
        assert!(
            active_entries
                .iter()
                .all(|&(_, scene_index)| resident_scene_indices.contains(&scene_index)),
            "non-resident source splats must never enter the active draw set"
        );
    }

    #[derive(Debug, Clone, Copy)]
    struct ImageParityMetrics {
        mean_abs_rgb: f64,
        frac_pixels_over_3_255: f64,
        max_abs_rgb: f64,
        alpha_mismatch_pixels: u64,
    }

    fn rgba_image_parity_metrics(direct: &[u8], packed: &[u8]) -> ImageParityMetrics {
        assert_eq!(direct.len(), packed.len());
        assert_eq!(direct.len() % 4, 0);
        let pixels = direct.len() / 4;
        let mut sum = 0.0_f64;
        let mut pixels_over = 0_u64;
        let mut max_abs = 0.0_f64;
        let mut alpha_mismatch_pixels = 0_u64;
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
            alpha_mismatch_pixels += u64::from(direct[base + 3] != packed[base + 3]);
        }
        ImageParityMetrics {
            mean_abs_rgb: sum / ((pixels * 3) as f64).max(1.0),
            frac_pixels_over_3_255: (pixels_over as f64) / (pixels as f64).max(1.0),
            max_abs_rgb: max_abs,
            alpha_mismatch_pixels,
        }
    }

    fn assert_image_parity(label: &str, first: &[u8], second: &[u8]) -> ImageParityMetrics {
        let metrics = rgba_image_parity_metrics(first, second);
        eprintln!(
            "{label}: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6} alpha_mismatch_pixels={}",
            metrics.mean_abs_rgb,
            metrics.frac_pixels_over_3_255,
            metrics.max_abs_rgb,
            metrics.alpha_mismatch_pixels,
        );
        assert!(
            metrics.mean_abs_rgb <= 1.0 / 255.0,
            "{label} mean abs RGB {:.6} exceeded 1/255",
            metrics.mean_abs_rgb
        );
        assert!(
            metrics.frac_pixels_over_3_255 <= 0.001,
            "{label} frac over 3/255 {:.6} exceeded 0.1%",
            metrics.frac_pixels_over_3_255
        );
        assert_eq!(
            metrics.alpha_mismatch_pixels, 0,
            "{label} must preserve every RGBA8 alpha byte"
        );
        metrics
    }

    #[test]
    fn image_parity_threshold_counts_pixels_not_channels() {
        let direct = [0_u8, 0, 0, 255, 0, 0, 0, 255];
        let packed = [4_u8, 0, 0, 255, 0, 0, 0, 255];
        let metrics = rgba_image_parity_metrics(&direct, &packed);
        assert_eq!(metrics.frac_pixels_over_3_255, 0.5);
        assert_eq!(metrics.alpha_mismatch_pixels, 0);
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
        let config = test_config(128);
        let Some(mut reference) = test_renderer(config, "{label} stale-order quality") else {
            return;
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
    fn packed_renderer_consumes_wide_source_and_retains_exact_sort_positions() {
        let source = build_scene();
        let expected_positions = source.positions.clone();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(source).unwrap();

        assert!(renderer.scene().is_none());
        assert!(renderer.resident_scene().is_some());
        assert_eq!(renderer.scene_len(), Some(expected_positions.len()));
        assert_eq!(renderer.scene_sh_degree(), Some(0));
        assert_eq!(renderer.positions(), Some(expected_positions.as_slice()));
        assert!(renderer.world_covariances.is_none());
        assert!(renderer.world_covariance_terms.is_none());
        assert!(renderer.alpha_values.is_none());

        let stats = renderer
            .build_surface_sorted_indices_with_sort_refresh(&Camera::default(), true)
            .unwrap();
        assert_eq!(stats.visible_count as usize, expected_positions.len());
        assert_eq!(stats.drawn_count, stats.visible_count);

        // A compact-only production load cannot be losslessly reconstructed
        // into the float32 Direct oracle merely by flipping a benchmark knob.
        renderer.set_geometry_path(super::GeometryPath::SortedIndexDirect);
        assert!(renderer.world_covariances.is_none());
        assert!(renderer.world_covariance_terms.is_none());
        assert!(renderer.alpha_values.is_none());
        assert!(renderer.resident_scene().is_some());
    }

    #[test]
    fn renderer_geometry_candidate_is_unpublished_until_infallible_commit() {
        let source = build_scene();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(source).unwrap();
        assert_eq!(
            renderer.geometry_path(),
            super::GeometryPath::SortedIndexDirect
        );
        assert!(renderer.world_covariances.is_some());

        let candidate = renderer
            .prepare_geometry_path_candidate(super::GeometryPath::PackedAtlas)
            .unwrap()
            .expect("changed path candidate");

        assert_eq!(
            renderer.geometry_path(),
            super::GeometryPath::SortedIndexDirect
        );
        assert!(renderer.world_covariances.is_some());
        assert!(renderer.scene().is_some());

        renderer.publish_geometry_path_candidate(candidate);
        assert_eq!(renderer.geometry_path(), super::GeometryPath::PackedAtlas);
        assert!(renderer.world_covariances.is_none());
        assert!(renderer.world_covariance_terms.is_none());
        assert!(renderer.alpha_values.is_none());
        assert!(renderer.scene().is_some());
    }

    #[test]
    fn direct_candidate_prepares_derived_data_without_mutating_paged_state() {
        let source = build_scene();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(source).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PagedActiveAtlas);
        assert!(renderer.spatial_pages.is_some());

        let candidate = renderer
            .prepare_geometry_path_candidate(super::GeometryPath::SortedIndexDirect)
            .unwrap()
            .expect("changed path candidate");

        assert_eq!(
            renderer.geometry_path(),
            super::GeometryPath::PagedActiveAtlas
        );
        assert!(renderer.spatial_pages.is_some());
        assert!(renderer.world_covariances.is_none());

        renderer.publish_geometry_path_candidate(candidate);
        assert_eq!(
            renderer.geometry_path(),
            super::GeometryPath::SortedIndexDirect
        );
        assert!(renderer.spatial_pages.is_none());
        assert!(renderer.world_covariances.is_some());
        assert!(renderer.world_covariance_terms.is_some());
        assert!(renderer.alpha_values.is_some());
    }

    #[test]
    fn packed_surface_handoff_releases_only_upload_planes() {
        let source = build_scene();
        let expected_positions = source.positions.clone();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(source).unwrap();

        let (before_order, _) = renderer
            .build_sorted_indices(&Camera::default())
            .expect("pre-handoff CPU order");
        let before_workspace = (
            renderer.cpu_order_engine.buffer_state(),
            renderer.preprocess_indices.capacity(),
        );
        let before = renderer
            .resident_scene()
            .unwrap()
            .cpu_byte_accounting()
            .unwrap();
        assert!(renderer.resident_scene().unwrap().has_upload_staging());

        let released = renderer
            .finish_surface_upload_handoff(super::GeometryPath::PackedAtlas)
            .expect("successful presenter handoff");
        let resident = renderer.resident_scene().unwrap();
        let after = resident.cpu_byte_accounting().unwrap();

        assert_eq!(released, before.upload_staging_bytes);
        assert_eq!(after.upload_staging_bytes, 0);
        assert_eq!(after.total_payload_bytes, before.exact_position_bytes);
        assert_eq!(resident.report.source_count, expected_positions.len());
        assert_eq!(resident.report.encoded_count, expected_positions.len());
        assert_eq!(renderer.positions(), Some(expected_positions.as_slice()));
        assert_eq!(renderer.scene_len(), Some(expected_positions.len()));
        assert!(renderer.has_scene());
        assert_eq!(
            (
                renderer.cpu_order_engine.buffer_state(),
                renderer.preprocess_indices.capacity(),
            ),
            before_workspace
        );

        let (after_order, _) = renderer
            .build_sorted_indices(&Camera::default())
            .expect("post-handoff CPU order");
        assert_eq!(after_order, before_order);
    }

    #[test]
    fn failed_surface_handoff_validation_keeps_every_upload_plane() {
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();
        let before = renderer
            .resident_scene()
            .unwrap()
            .cpu_byte_accounting()
            .unwrap();
        renderer
            .resident_scene_cpu
            .as_mut()
            .unwrap()
            .report
            .encoded_count -= 1;

        assert!(matches!(
            renderer.finish_surface_upload_handoff(super::GeometryPath::PackedAtlas),
            Err(RendererError::ResidentScene(
                super::ResidentSceneError::CountMismatch { .. }
            ))
        ));
        let resident = renderer.resident_scene().unwrap();
        assert!(resident.has_upload_staging());
        assert_eq!(resident.cpu_byte_accounting().unwrap(), before);
    }

    #[test]
    fn failed_presenter_construction_does_not_release_packed_staging() {
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();
        let before = renderer
            .resident_scene()
            .unwrap()
            .cpu_byte_accounting()
            .unwrap();

        let result = try_prepare_then_commit(
            &mut renderer,
            |_| Err::<(), _>("injected GPU resource construction failure"),
            |renderer, ()| {
                renderer
                    .finish_surface_upload_handoff(super::GeometryPath::PackedAtlas)
                    .expect("commit handoff");
            },
        );

        assert_eq!(result, Err("injected GPU resource construction failure"));
        let resident = renderer.resident_scene().unwrap();
        assert!(resident.has_upload_staging());
        assert_eq!(resident.cpu_byte_accounting().unwrap(), before);
    }

    #[test]
    fn packed_offscreen_retains_staging_for_resource_recreation_after_path_toggle() {
        let Some(mut renderer) = test_renderer(test_config(64), "packed staging ownership") else {
            return;
        };
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();
        let before = renderer
            .resident_scene()
            .unwrap()
            .cpu_byte_accounting()
            .unwrap();
        assert_eq!(before.exact_position_bytes, 24);
        assert_eq!(before.upload_staging_bytes, 176);
        assert_eq!(before.total_payload_bytes, 200);

        renderer.render_frame(&Camera::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::SortedIndexDirect);
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer
            .render_frame(&Camera::default())
            .expect("retained staging must recreate cleared offscreen GPU resources");

        let resident = renderer.resident_scene().unwrap();
        assert!(resident.has_upload_staging());
        assert_eq!(resident.cpu_byte_accounting().unwrap(), before);
    }

    #[test]
    fn resident_scene_load_validates_before_replacing_renderer_state() {
        let first_source = build_scene();
        let first_positions = first_source.positions.clone();
        let first_resident = super::ResidentSceneCpu::encode(&first_source).unwrap();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PackedAtlas);
        renderer.load_resident_scene(first_resident).unwrap();

        let mut invalid = super::ResidentSceneCpu::encode(&build_scene()).unwrap();
        invalid.report.encoded_count = 1;
        assert!(matches!(
            renderer.load_resident_scene(invalid),
            Err(RendererError::ResidentScene(
                super::ResidentSceneError::CountMismatch {
                    expected: 2,
                    actual: 1,
                }
            ))
        ));
        assert_eq!(renderer.positions(), Some(first_positions.as_slice()));
        assert_eq!(renderer.scene_len(), Some(first_positions.len()));

        let direct_resident = super::ResidentSceneCpu::encode(&build_scene()).unwrap();
        let mut direct = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        assert!(matches!(
            direct.load_resident_scene(direct_resident),
            Err(RendererError::InvalidScene)
        ));
        assert!(!direct.has_scene());
    }

    #[test]
    fn paged_renderer_preselection_builds_pages_without_direct_cpu_caches() {
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(super::GeometryPath::PagedActiveAtlas);
        renderer.load_scene(build_scene()).unwrap();

        assert_eq!(
            renderer.geometry_path(),
            super::GeometryPath::PagedActiveAtlas
        );
        assert!(renderer.world_covariances.is_none());
        assert!(renderer.world_covariance_terms.is_none());
        assert!(renderer.alpha_values.is_none());
        assert!(
            renderer
                .spatial_pages
                .as_ref()
                .is_some_and(|pages| !pages.pages.is_empty())
        );
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_degree0_scene() {
        let Some(pair) = render_direct_packed(build_scene(), 128, "packed image parity") else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        assert_image_parity("packed image parity", &pair.first_rgba, &pair.second_rgba);
    }

    #[test]
    fn reference_blend_contract_caps_alpha_and_preserves_sh_highlights_above_one() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![100.0],
            scale_xyz: vec![[-4.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            // SH0 red = 0.5 + C0 * 4 ~= 1.628. The reference only
            // clamps negative SH colors; an incorrect UNORM/source clamp
            // would keep the brightest red below the target maximum.
            color_dc: vec![[4.0, 0.0, 0.0]],
            sh_degree: 0,
            sh_rest: None,
        };
        // Odd dimensions put NDC (0,0) exactly on a pixel center.
        let Some(pair) = render_direct_packed(scene, 65, "reference blend contract") else {
            return;
        };
        assert_image_parity(
            "reference blend direct vs resident",
            &pair.first_rgba,
            &pair.second_rgba,
        );
        for rgba in [&pair.first_rgba, &pair.second_rgba] {
            let brightest = rgba
                .chunks_exact(4)
                .max_by_key(|pixel| pixel[3])
                .expect("render target has pixels");
            assert_eq!(
                brightest[0], 255,
                "SH highlight above one was clamped early"
            );
            assert!(
                (125..=127).contains(&brightest[1]),
                "brightest={brightest:?}"
            );
            assert!(
                (125..=127).contains(&brightest[2]),
                "brightest={brightest:?}"
            );
            assert!(
                (252..=253).contains(&brightest[3]),
                "brightest={brightest:?}"
            );
        }
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_minimal_ascii() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/minimal_ascii.ply");
        let loaded = gsplat_io_ply::load_ply(&path).expect("load minimal_ascii");
        let Some(pair) = render_direct_packed(loaded.scene, 128, "minimal_ascii image parity")
        else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        assert_image_parity(
            "minimal_ascii packed parity",
            &pair.first_rgba,
            &pair.second_rgba,
        );
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

        let config = test_config(128);
        let Some(mut from_spz) = test_renderer(config, "PLY-vs-SPZ image parity") else {
            return;
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

    fn synthetic_degree3_scene() -> SceneBuffers {
        let count = 64usize;
        SceneBuffers {
            positions: (0..count)
                .map(|i| {
                    let t = i as f32 / count as f32;
                    Vec3f::new((t - 0.5) * 2.0, 0.0, 1.5 + (i / 16) as f32 * 0.1)
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
        }
    }

    fn paged_eviction_scene() -> SceneBuffers {
        let positions: Vec<_> = (0..3)
            .flat_map(|z| {
                (0..3).flat_map(move |y| {
                    (0..3).map(move |x| {
                        Vec3f::new(x as f32 * 2.0 - 2.0, y as f32 * 2.0 - 2.0, z as f32 + 1.0)
                    })
                })
            })
            .collect();
        let count = positions.len();
        SceneBuffers {
            positions,
            opacity: vec![2.0; count],
            scale_xyz: vec![[-3.0, -3.0, -3.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.2, 0.0, -0.1]; count],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn packed_vs_direct_image_parity_gate_on_synthetic_degree3() {
        let Some(pair) =
            render_direct_packed(synthetic_degree3_scene(), 128, "synthetic degree3 parity")
        else {
            return;
        };
        assert_image_parity(
            "synthetic degree3 packed parity",
            &pair.first_rgba,
            &pair.second_rgba,
        );
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
        let Some(pair) = render_direct_packed(loaded.scene, 128, "kitsune image parity") else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        let metrics = assert_image_parity(
            "kitsune degree-3 parity",
            &pair.first_rgba,
            &pair.second_rgba,
        );
        // Also report alpha-channel MAE to separate coverage vs color error.
        let mut sum_a = 0.0_f64;
        let mut n = 0_u64;
        for (d, p) in pair
            .first_rgba
            .chunks_exact(4)
            .zip(pair.second_rgba.chunks_exact(4))
        {
            sum_a += (d[3] as f64 - p[3] as f64).abs() / 255.0;
            n += 1;
        }
        let mut sum_direct_a = 0.0_f64;
        let mut sum_packed_a = 0.0_f64;
        for (d, p) in pair
            .first_rgba
            .chunks_exact(4)
            .zip(pair.second_rgba.chunks_exact(4))
        {
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
            pair.first_stats.visible_count
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
        let Some(pair) = render_direct_packed(loaded.scene, 128, "Flowers image parity") else {
            return;
        };
        assert_eq!(
            pair.first_stats.visible_count,
            pair.second_stats.visible_count
        );
        assert_eq!(pair.first_stats.drawn_count, pair.second_stats.drawn_count);
        assert!(
            pair.first_stats.visible_count > 0,
            "Flowers parity camera must see splats"
        );
        assert_image_parity(
            "Flowers degree-3 parity",
            &pair.first_rgba,
            &pair.second_rgba,
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_visibility_preprocess_matches_sequential_source_order() {
        let count = super::PARALLEL_PREPROCESS_THRESHOLD + 37;
        let positions = (0..count)
            .map(|index| {
                let z = if index % 11 == 0 {
                    -1.0
                } else if index % 13 == 0 {
                    2_000.0
                } else {
                    0.5 + (index % 97) as f32 * 0.03125
                };
                Vec3f::new((index % 17) as f32 * 0.01, (index % 23) as f32 * -0.01, z)
            })
            .collect::<Vec<_>>();
        let camera = Camera::default();
        let mut expected_keys = Vec::new();
        let mut expected_indices = Vec::new();
        super::preprocess_positions_visible_into(
            &positions,
            &camera,
            &mut expected_keys,
            &mut expected_indices,
        )
        .expect("sequential preprocessing");

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .expect("four-thread preprocessing pool");
        let mut actual_keys = Vec::new();
        let mut actual_indices = Vec::new();
        let mut chunks = Vec::new();
        pool.install(|| {
            super::preprocess_positions_visible_into_parallel(
                &positions,
                &camera,
                &mut actual_keys,
                &mut actual_indices,
                &mut chunks,
            )
        })
        .expect("parallel preprocessing");

        assert_eq!(actual_keys, expected_keys);
        assert_eq!(actual_indices, expected_indices);
        assert!(actual_indices.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(chunks.len(), 4);
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
    fn packed_scene_preflight_accessor_requires_a_loaded_scene() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();

        let error = renderer.current_packed_scene_preflight().unwrap_err();

        assert!(matches!(error, super::RendererError::SceneNotLoaded));
    }

    #[test]
    fn packed_scene_preflight_accessor_does_not_guess_surface_device_limits() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer.load_scene(build_scene()).unwrap();

        let error = renderer.current_packed_scene_preflight().unwrap_err();

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

        assert!(matches!(
            err,
            RendererError::GpuDimensionsUnsupported {
                width: 4096,
                height: 2160,
                max_dimension: 2048,
            }
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_preserve_adapter_headroom_for_later_packed_scenes() {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_texture_dimension_2d = 8192;
        adapter_limits.max_storage_buffer_binding_size = 512 << 20;
        adapter_limits.max_buffer_size = 1024 << 20;
        adapter_limits.max_storage_buffers_per_shader_stage = 8;
        let config = RendererConfig {
            width: 4096,
            height: 2160,
            mode: RenderMode::SortedAlpha,
        };

        let requested = offscreen_device_limits(&config, &adapter_limits).unwrap();

        assert_eq!(requested.max_texture_dimension_2d, 8192);
        assert_eq!(requested.max_storage_buffer_binding_size, 512 << 20);
        assert_eq!(requested.max_buffer_size, 1024 << 20);
        assert_eq!(requested.max_storage_buffers_per_shader_stage, 8);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_keep_direct_available_below_resident_binding_count() {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_texture_dimension_2d = 8192;
        adapter_limits.max_storage_buffers_per_shader_stage = 7;
        let config = RendererConfig {
            width: 640,
            height: 480,
            mode: RenderMode::SortedAlpha,
        };

        let requested = offscreen_device_limits(&config, &adapter_limits).unwrap();

        assert_eq!(requested.max_storage_buffers_per_shader_stage, 7);
        let packed = packed_scene_preflight_with_limits(1, 3, &requested).unwrap();
        assert_eq!(
            packed.failure,
            Some(PackedScenePreflightFailure::StorageBindingCount {
                required: 8,
                available: 7,
            })
        );
    }

    fn limits_with_storage_binding_limit(bytes: u32) -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffer_binding_size = bytes;
        limits.max_buffer_size = u64::from(bytes);
        limits.max_storage_buffers_per_shader_stage =
            super::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
        limits
    }

    #[test]
    fn default_surface_path_stays_direct_while_preflight_reports_oversized_scenes() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let small = direct_scene_preflight(279_199, 3, &limits).unwrap();
        assert_eq!(small.path, DirectScenePath::Direct);
        assert_eq!(GeometryPath::default(), GeometryPath::SortedIndexDirect);

        let oversized = direct_scene_preflight(3_454_040, 3, &limits).unwrap();
        assert_eq!(oversized.path, DirectScenePath::ActiveAtlasRequired);
        assert_eq!(GeometryPath::default(), GeometryPath::SortedIndexDirect);
    }

    #[test]
    fn surface_resource_plan_selects_fixed_slots_for_over_direct_limit_nandi() {
        let mut limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        limits.max_texture_dimension_2d = 8192;
        let scene_splats = 3_454_040_usize;
        let page_capacity = 65_536_usize;
        let page_count = scene_splats.div_ceil(page_capacity);

        let direct = super::surface_resource_plan(
            GeometryPath::SortedIndexDirect,
            scene_splats,
            3,
            0,
            0,
            640,
            480,
            &limits,
        )
        .unwrap();
        assert_eq!(
            direct.direct_preflight.path,
            DirectScenePath::ActiveAtlasRequired
        );
        assert_eq!(
            direct.direct_preflight.limiting_resource,
            DirectSceneResource::ShRest
        );
        assert_eq!(
            direct.direct_preflight.requirements[2].required_bytes,
            621_727_200
        );
        assert!(matches!(
            direct.validate_selected_path(&limits),
            Err(super::SurfacePresenterError::DirectScene(
                DirectSceneError::ResourceLimitExceeded(_)
            ))
        ));

        let paged = super::surface_resource_plan(
            GeometryPath::PagedActiveAtlas,
            scene_splats,
            3,
            page_count,
            page_capacity,
            640,
            480,
            &limits,
        )
        .unwrap();
        assert_eq!(page_count, 53);
        assert_eq!(page_capacity, 65_536);
        let slot_count = page_count.min(super::DEFAULT_PAGED_ATLAS_SLOTS);
        assert_eq!(slot_count, 4);
        assert!(slot_count < page_count);
        let resident_capacity = slot_count.saturating_mul(page_capacity);
        assert_eq!(resident_capacity / page_capacity, slot_count);
        assert_eq!(resident_capacity, 262_144);
        assert!(resident_capacity < scene_splats);
        assert_eq!(paged.packed_preflight.path, PackedScenePath::PackedAtlas);
        assert_eq!(paged.packed_preflight.splat_count, scene_splats as u64);
        assert_eq!(paged.packed_preflight.resident_gpu.sh_plane_count, 4);
        assert_eq!(paged.paged_plan.sorted_indices_bytes, 1_048_576);
        assert_eq!(paged.paged_plan.hot_record_storage_bytes, 5_242_880);
        assert!(paged.required_texture_dimension <= 8192);
        paged.validate_selected_path(&limits).unwrap();
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
    fn covariance_low_pass_matches_reference_point_three_pixel_squared() {
        let config = RendererConfig {
            width: 100,
            height: 200,
            mode: RenderMode::SortedAlpha,
        };
        let params = super::InstanceBuildParams::new(&Camera::default(), config).unwrap();
        let px_ndc_x = 2.0 / config.width as f32;
        let px_ndc_y = 2.0 / config.height as f32;
        assert_eq!(params.blur_cov_x, 0.3 * px_ndc_x.powi(2));
        assert_eq!(params.blur_cov_y, 0.3 * px_ndc_y.powi(2));
        assert_ne!(params.blur_cov_x, (0.3 * px_ndc_x).powi(2));
    }

    #[test]
    fn covariance_preserves_sub_micrometer_finite_scales_without_flooring() {
        let scale = (-14.0_f32).exp();
        let covariance =
            super::world_covariance_from_source([-14.0, -14.0, -14.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(covariance[0][0], scale * scale);
        assert!(covariance[0][0] < 1.0e-12);
        assert!(super::log_scale_has_finite_nonzero_covariance([
            -14.0, -14.0, -14.0
        ]));
        assert!(!super::log_scale_has_finite_nonzero_covariance([
            -200.0, 0.0, 0.0
        ]));
        assert!(!super::log_scale_has_finite_nonzero_covariance([
            100.0, 0.0, 0.0
        ]));
    }

    #[test]
    fn direct_scene_rejects_unrepresentable_covariance_and_zero_rotation() {
        let make_scene = |scale_xyz, rotation_xyzw| SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0)],
            opacity: vec![1.0],
            scale_xyz: vec![scale_xyz],
            rotation_xyzw: vec![rotation_xyzw],
            color_dc: vec![[0.0; 3]],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        assert!(matches!(
            renderer.load_scene(make_scene([-200.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])),
            Err(RendererError::InvalidScene)
        ));
        assert!(matches!(
            renderer.load_scene(make_scene([0.0; 3], [0.0; 4])),
            Err(RendererError::InvalidScene)
        ));
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
