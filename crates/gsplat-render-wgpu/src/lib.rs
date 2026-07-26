#![deny(clippy::debug_assert_with_mut_call)]

//! WGPU renderer with a SortedAlpha reference path.

mod api;
mod cpu;
mod cpu_order;
mod data;
mod direct_gpu_order;
mod direct_scene_gpu;
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
pub use api::{GeometryPath, PreprocessOutput, SurfaceRasterExecutionPlan};
#[cfg(test)]
use cpu::reference::{
    InstanceBuildParams, build_instances, ellipse_axes_from_covariance, project_covariance_to_ndc,
    project_world_covariance_terms_to_ndc, quat_normalize, world_to_camera_with_view_rot,
};
pub(crate) use cpu::reference::{
    log_scale_has_finite_nonzero_covariance, rotation_has_finite_nonzero_norm,
    world_covariance_from_source,
};
#[cfg(test)]
use cpu::reference::{precompute_alpha_values, precompute_world_covariances};
use cpu::reference::{quat_inverse, quat_to_mat3, sh_color_unchecked};
use cpu_order::CpuOrderEngine;
#[cfg(test)]
pub(crate) use cpu_order::world_to_camera_depth_with_view_row;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use cpu_order::{
    PARALLEL_PREPROCESS_THRESHOLD, preprocess_positions_visible_into_parallel,
};
pub(crate) use cpu_order::{
    is_visible, preprocess_paged_visible_into, preprocess_positions_visible_into,
};
#[cfg(test)]
pub(crate) use data::CameraCovarianceTerms;
pub(crate) use data::{CpuPositionView, GpuSortPair, GpuSurfaceRenderParams, ShColorLayout};
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
pub(crate) use spatial_pages::SpatialPageSet;
pub use surface::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsCounts, SurfaceCurrentStatsFailure,
    SurfaceCurrentStatsFrameIdentity, SurfaceCurrentStatsJoinIdentity, SurfaceCurrentStatsPlan,
    SurfaceCurrentStatsPoll, SurfaceCurrentStatsReceipt, SurfaceCurrentStatsRequest,
    SurfaceCurrentStatsSubmission, SurfaceCurrentStatsSubmissionReceipt,
    SurfaceCurrentStatsTerminal, SurfaceCurrentStatsUnsampledReason,
};
#[cfg(test)]
use surface::{standalone_paged_runtime::StandalonePagedRuntime, try_prepare_then_commit};
#[cfg(test)]
use surface_presenter::surface_resource_plan;
pub use surface_presenter::{SurfaceFrameCapture, SurfacePresenter};
pub use surface_session::{
    SurfaceAdaptiveGpuFailureReason, SurfaceAdaptivePendingSample, SurfaceAdaptiveState,
    SurfaceFrameOutput, SurfaceFrameTimings, SurfaceGpuProducerMeasurementSubmission,
    SurfaceGpuProducerMeasurementUnsampledReason, SurfaceOrderBackend, SurfaceOrderBackendUsed,
    SurfaceOrderMeasurementSubmission, SurfaceOrderMeasurementUnsampledReason,
    SurfaceProjectedDrawAdaptivePendingSample, SurfaceProjectedDrawAdaptiveState,
    SurfaceProjectedDrawMeasurementSubmission, SurfaceProjectedDrawMeasurementUnsampledReason,
    SurfaceProjectedDrawPolicy, SurfaceRenderSession, SurfaceSortSchedule,
};
const DEFAULT_PAGED_ATLAS_SLOTS: usize = 4;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers};
use gsplat_sort::SortError;
use thiserror::Error;

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

/// Advances callbacks for queue work that already exists without encoding or
/// submitting another command buffer. A timeout is an ordinary pending result;
/// callers retain their own finite drain bound.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn pump_device_receipt_callbacks(
    device: &wgpu::Device,
    timeout: Duration,
) -> Result<bool, RendererError> {
    match device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(timeout),
    }) {
        Ok(status) => Ok(status.wait_finished()),
        Err(wgpu::PollError::Timeout) => Ok(false),
        Err(wgpu::PollError::WrongSubmissionIndex(_, _)) => Err(RendererError::GpuWait),
    }
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
            | Self::ResidentGpu(_) => ErrorCode::Unsupported,
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
    #[error(
        "the requested runtime surface geometry transition is unsupported; transitions entering or leaving Packed are disabled, and Paged remains a constructor-time diagnostic"
    )]
    SurfaceGeometrySwitchUnsupported,
    #[error(
        "standalone Packed surface presenters are unsupported; construct Packed through SurfaceRenderSession::from_* so the session owns the Exact surface host"
    )]
    StandalonePackedPresenterUnsupported,
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
            | Self::SurfaceGeometrySwitchUnsupported
            | Self::StandalonePackedPresenterUnsupported
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
            | Self::ResidentGpu(_) => ErrorCode::Unsupported,
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
    offscreen_host: Option<renderer::offscreen_host::OffscreenHost>,
    scene_state: renderer::scene_state::RendererSceneState,
    /// Sole complete Exact runtime for Packed rendering. Offscreen and
    /// Surface hosts supply different targets but never own a second scene,
    /// PlanSet, controller, generation ledger, sampler, or raster graph.
    exact_offscreen_runtime: Option<renderer::PreparedRuntimeSlot>,
    /// Direct Surface order prepared for one frame attempt. It is deliberately
    /// separate from `scene_state.preprocess_indices`, which is the last
    /// successfully presented order exposed by `current_sorted_indices()`.
    surface_attempt_order: Option<Vec<u32>>,
    /// Statistics for the same unpublished Direct/Paged Surface attempt.
    surface_attempt_stats: Option<FrameStats>,
    last_stats: FrameStats,
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
fn default_spatial_pages(scene: &SceneBuffers) -> SpatialPageSet {
    renderer::scene_state::default_spatial_pages(scene)
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

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::Arc;

    use crate::spatial_pages::PageId;
    use gsplat_core::{
        Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f,
    };

    #[cfg(not(target_arch = "wasm32"))]
    use super::renderer::offscreen_host::offscreen_device_limits;
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

        let host = renderer.offscreen_host.as_ref().expect("offscreen owner");
        let slot = renderer
            .exact_offscreen_runtime
            .as_ref()
            .expect("Packed Exact runtime");
        assert!(slot.same_gpu_arc_owner(host.device(), host.queue()));
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
            .offscreen_host
            .as_ref()
            .unwrap()
            .max_texture_dimension_2d();
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
        let device = Arc::clone(renderer.offscreen_host.as_ref().unwrap().device());
        let scoped_error = renderer
            .offscreen_host
            .as_mut()
            .unwrap()
            .ensure_output_target_with_device_for_test(&device, 0, config.height);
        assert!(matches!(
            scoped_error,
            Err(RendererError::GpuDeviceCreation)
        ));
        assert_eq!(
            renderer.offscreen_host.as_ref().unwrap().target_size(),
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
            let host = renderer.offscreen_host.as_ref().unwrap();
            let active_set = host.paged_active_set_for_test().unwrap();
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
            let host = renderer.offscreen_host.as_ref().unwrap();
            let active_set = host.paged_active_set_for_test().unwrap();
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
                .offscreen_host
                .as_ref()
                .unwrap()
                .paged_active_set_for_test()
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
        let pages = super::default_spatial_pages(&scene);
        let page_count = pages.page_count();
        let page_capacity = pages.page_capacity;
        assert!(page_count > super::DEFAULT_PAGED_ATLAS_SLOTS);
        let mut runtime =
            super::StandalonePagedRuntime::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let prepared = runtime
            .prepare_scene_candidate(&device, &scene, pages)
            .unwrap();
        assert_eq!(prepared.addressable_splat_count(), 4 * page_capacity);
        assert!(prepared.addressable_splat_count() < scene.len());
        runtime.publish_scene(prepared);

        for position in [
            Vec3f::new(-3.0, -3.0, 0.0),
            Vec3f::new(-2.9, -3.0, 0.0),
            Vec3f::new(-2.8, -2.9, 0.0),
        ] {
            let mut camera = Camera::default();
            camera.pose.position = position;
            runtime
                .prepare_frame(&queue, &scene, &camera, config.width, config.height)
                .unwrap();
            assert!(
                runtime.instance_count() > 0,
                "Surface paged runtime must prepare non-zero draw"
            );
            assert_eq!(
                runtime.slot_counts(),
                Some((
                    super::DEFAULT_PAGED_ATLAS_SLOTS,
                    super::DEFAULT_PAGED_ATLAS_SLOTS,
                ))
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
            let host = renderer.offscreen_host.as_ref().unwrap();
            let active_set = host.paged_active_set_for_test().unwrap();
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
        assert!(renderer.world_covariances().is_none());
        assert!(renderer.direct_scene_cpu_inputs().is_none());

        let stats = renderer
            .build_surface_sorted_indices_with_sort_refresh(&Camera::default(), true)
            .unwrap();
        assert_eq!(stats.visible_count as usize, expected_positions.len());
        assert_eq!(stats.drawn_count, stats.visible_count);

        // A compact-only production load cannot be losslessly reconstructed
        // into the float32 Direct oracle merely by flipping a benchmark knob.
        renderer.set_geometry_path(super::GeometryPath::SortedIndexDirect);
        assert!(renderer.world_covariances().is_none());
        assert!(renderer.direct_scene_cpu_inputs().is_none());
        assert!(renderer.resident_scene().is_some());
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
            renderer.preprocess_capacity(),
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
                renderer.preprocess_capacity(),
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
            .scene_state
            .resident_upload_mut()
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
    fn invalid_wide_replacement_preserves_scene_caches_order_and_public_stats() {
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(build_scene()).unwrap();
        renderer
            .build_sorted_indices(&Camera::default())
            .expect("establish published Direct state");

        let positions = renderer.positions().unwrap().as_ptr();
        let world_covariances = renderer.world_covariances().unwrap().as_ptr();
        let (_, world_covariance_terms, alpha_values) = renderer.direct_scene_cpu_inputs().unwrap();
        let world_covariance_terms = world_covariance_terms.as_ptr();
        let alpha_values = alpha_values.as_ptr();
        let order = renderer.current_sorted_indices().to_vec();
        let stats = renderer.last_stats();

        let mut invalid = build_scene();
        invalid.rotation_xyzw[0] = [0.0; 4];
        assert!(matches!(
            renderer.load_scene(invalid),
            Err(RendererError::InvalidScene)
        ));

        assert_eq!(renderer.positions().unwrap().as_ptr(), positions);
        assert_eq!(
            renderer.world_covariances().unwrap().as_ptr(),
            world_covariances
        );
        let (_, retained_terms, retained_alpha) = renderer.direct_scene_cpu_inputs().unwrap();
        assert_eq!(retained_terms.as_ptr(), world_covariance_terms);
        assert_eq!(retained_alpha.as_ptr(), alpha_values);
        assert_eq!(renderer.current_sorted_indices(), order);
        assert_eq!(renderer.last_stats(), stats);
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
        assert!(renderer.world_covariances().is_none());
        assert!(renderer.direct_scene_cpu_inputs().is_none());
        assert!(
            renderer
                .spatial_pages()
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
