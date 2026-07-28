//! Stable C ABI surface for mobile wrappers.

mod current_stats_v1;
mod current_stats_v2;

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Display;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::NonNull;
use std::time::Duration;

#[cfg(any(target_os = "android", target_os = "ios"))]
use gsplat_core::camera_trace::CameraTrace;
use gsplat_core::{
    Camera, CameraIntrinsics, CameraPose, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR,
    GSPLAT_API_VERSION_MINOR, RenderMode, RendererConfig, Vec3f,
};
use gsplat_io_ply::{
    DecodedPlySplat, PlyLoadError, PlySceneSummary, load_ply, load_ply_summary, visit_ply_splats,
};
use gsplat_render_wgpu::{
    GeometryPath, Renderer, RendererError, ResidentSceneBuilder, ResidentSceneError,
    ResidentSourceSplat, SurfaceAdaptiveGpuFailureReason, SurfaceAdaptiveState,
    SurfaceCompatibilityChannel, SurfaceCompatibilityCountFamily, SurfaceCompatibilityCounts,
    SurfaceCompatibilityCountsTake, SurfaceCompatibilityOrderCpuSuccess,
    SurfaceCompatibilityOrderFailure, SurfaceCompatibilityOrderGpuSuccess,
    SurfaceCompatibilityOrderIssueContext, SurfaceCompatibilityOrderSubmission,
    SurfaceCompatibilityProducerSubmission, SurfaceCompatibilityProjectedFailure,
    SurfaceCompatibilityProjectedSubmission, SurfaceCompatibilityProjectedSuccess,
    SurfaceCompatibilitySubmission, SurfaceCompatibilityTerminal, SurfaceCompatibilityTerminalPoll,
    SurfaceCompatibilityTerminalSelector, SurfaceFrameOutput, SurfaceGpuOrderProducer,
    SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
    SurfaceGpuProducerMeasurementSubmission, SurfaceGpuProducerMeasurementUnsampledReason,
    SurfaceOrderBackend, SurfaceOrderBackendUsed, SurfaceOrderMeasurementFailureReason,
    SurfaceOrderMeasurementSubmission, SurfaceOrderMeasurementUnsampledReason,
    SurfaceProjectedDrawAdaptiveState, SurfaceProjectedDrawExecution,
    SurfaceProjectedDrawMeasurementFailureReason, SurfaceProjectedDrawMeasurementSubmission,
    SurfaceProjectedDrawMeasurementUnsampledReason, SurfaceProjectedDrawPolicy,
    SurfaceRenderSession, SurfaceTimingSource,
};

pub use current_stats_v1::{
    GsplatSurfaceCurrentStatsIdentityV1, GsplatSurfaceCurrentStatsPollV1,
    GsplatSurfaceCurrentStatsRequestV1, GsplatSurfaceCurrentStatsSubmissionV1,
};
use current_stats_v1::{
    SURFACE_CURRENT_STATS_ABI_VERSION_V1, surface_current_stats_poll_to_ffi,
    surface_current_stats_request_to_ffi, surface_current_stats_submission_to_ffi,
};
pub use current_stats_v2::GsplatSurfaceCurrentStatsPollV2;
use current_stats_v2::{
    SURFACE_CURRENT_STATS_ABI_VERSION_V2, surface_current_stats_poll_to_ffi_v2,
};

const SURFACE_CAMERA_MAX_PITCH: f32 = 1.45;
const SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER: f32 = 0.2;
const SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER: f32 = 20.0;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsplatConfig {
    pub width: u32,
    pub height: u32,
    pub mode: u32,
}

impl Default for GsplatConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            mode: RenderMode::SortedAlpha as u32,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsplatStats {
    pub frame_ms: f32,
    pub preprocess_ms: f32,
    pub sort_ms: f32,
    pub raster_ms: f32,
    pub visible_count: u32,
    pub drawn_count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GsplatSurfaceSortStats {
    pub camera_revision: u64,
    pub applied_order_revision: u64,
    pub scheduled_revision: u64,
    pub completed_revision: u64,
    pub presented_order_revision_lag: u32,
    pub observed_result_revision_lag: u32,
    pub flags: u32,
}

/// One completed, non-blocking GPU order receipt.
///
/// Optional floating-point fields are zero when their corresponding validity
/// bit is clear. This is a new additive type; the layout of
/// [`GsplatSurfaceSortStats`] remains unchanged.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GsplatSurfaceOrderMeasurement {
    pub ticket: u64,
    pub camera_revision: u64,
    pub gpu_preprocess_ms: f32,
    pub gpu_radix_ms: f32,
    pub gpu_order_ms: f32,
    pub gpu_complete_ms: f32,
    pub timestamp_period_ns: f32,
    pub visible_count: u32,
    pub drawn_count: u32,
    pub timing_source: u32,
    pub requested_backend: u32,
    pub actual_backend: u32,
    pub adaptive_state: u32,
    pub flags: u32,
}

/// One completed CPU order refresh measured through graphics-queue completion.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GsplatSurfaceCpuOrderMeasurement {
    pub ticket: u64,
    pub camera_revision: u64,
    pub preprocess_ms: f32,
    pub sort_ms: f32,
    pub frame_complete_ms: f32,
    pub requested_backend: u32,
    pub actual_backend: u32,
    pub adaptive_state: u32,
    pub flags: u32,
    pub reserved: u32,
}

/// Ticket-addressed V/C/D receipt shared by CPU and GPU terminal successes.
/// This additive type leaves both legacy measurement layouts unchanged.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GsplatSurfaceOrderCounts {
    pub ticket: u64,
    pub camera_revision: u64,
    pub visible_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub flags: u32,
}

/// Terminal failure receipt for one issued CPU or GPU order measurement ticket.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GsplatSurfaceOrderMeasurementFailure {
    pub ticket: u64,
    pub camera_revision: u64,
    pub reason: u32,
    pub requested_backend: u32,
    pub actual_backend: u32,
    pub adaptive_state: u32,
    pub flags: u32,
    pub reserved: u32,
}

/// Submission identity for the last successful Surface render call.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GsplatSurfaceOrderSubmission {
    pub ticket: u64,
    pub camera_revision: u64,
    pub requested_backend: u32,
    pub actual_backend: u32,
    pub adaptive_state: u32,
    pub flags: u32,
}

const SURFACE_PROJECTED_ABI_VERSION_V1: u32 = 1;

/// Versioned submission identity for the independent projected-draw policy.
///
/// Callers initialize `struct_size` and `version` before every getter call.
/// A ticket is public only when `TICKET_ISSUED` is set.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceProjectedSubmissionV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub requested_policy: u32,
    pub actual_execution: u32,
    pub order_backend: u32,
    pub adaptive_state: u32,
    pub flags: u32,
    pub reserved: u32,
}

impl Default for GsplatSurfaceProjectedSubmissionV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_PROJECTED_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            requested_policy: 0,
            actual_execution: 0,
            order_backend: 0,
            adaptive_state: 0,
            flags: 0,
            reserved: 0,
        }
    }
}

/// Versioned terminal success for one projected-draw ticket.
/// Exact V/C/D values are taken separately by the same ticket.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GsplatSurfaceProjectedMeasurementV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub projection_generation: u64,
    pub probe_generation: u64,
    pub frame_complete_ms: f32,
    pub execution: u32,
    pub order_backend: u32,
    pub flags: u32,
}

impl Default for GsplatSurfaceProjectedMeasurementV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_PROJECTED_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            projection_generation: 0,
            probe_generation: 0,
            frame_complete_ms: 0.0,
            execution: 0,
            order_backend: 0,
            flags: 0,
        }
    }
}

/// Versioned, ticket-addressed V/C/D receipt for projected-draw success.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceProjectedCountsV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub visible_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub flags: u32,
}

impl Default for GsplatSurfaceProjectedCountsV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_PROJECTED_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            visible_count: 0,
            contributor_count: 0,
            drawn_count: 0,
            flags: 0,
        }
    }
}

/// Versioned terminal failure for one projected-draw ticket.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceProjectedFailureV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub projection_generation: u64,
    pub probe_generation: u64,
    pub reason: u32,
    pub execution: u32,
    pub order_backend: u32,
    pub flags: u32,
}

impl Default for GsplatSurfaceProjectedFailureV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_PROJECTED_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            projection_generation: 0,
            probe_generation: 0,
            reason: 0,
            execution: 0,
            order_backend: 0,
            flags: 0,
        }
    }
}

const SURFACE_GPU_PRODUCER_ABI_VERSION_V1: u32 = 1;

/// Versioned submission identity for the optional Packed GPU-producer A/B lane.
///
/// The default renderer neither requests producer measurements nor allocates
/// FFI-side receipt storage. Callers must opt in explicitly after selecting a
/// producer under the strict Packed + ProjectedQuadsExact + Compact + GPU
/// benchmark context.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceGpuProducerSubmissionV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub requested_producer: u32,
    pub actual_producer: u32,
    pub order_backend: u32,
    pub projected_execution: u32,
    pub flags: u32,
    pub reserved: u32,
}

impl Default for GsplatSurfaceGpuProducerSubmissionV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_GPU_PRODUCER_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            requested_producer: 0,
            actual_producer: 0,
            order_backend: 0,
            projected_execution: 0,
            flags: 0,
            reserved: 0,
        }
    }
}

/// Terminal success for one producer ticket, including exact S/C/D counts.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GsplatSurfaceGpuProducerMeasurementV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub order_generation: u64,
    pub projection_generation: u64,
    pub frame_complete_ms: f32,
    pub producer: u32,
    pub source_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub draw_scope: u32,
    pub flags: u32,
    pub reserved: u32,
}

impl Default for GsplatSurfaceGpuProducerMeasurementV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_GPU_PRODUCER_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            order_generation: 0,
            projection_generation: 0,
            frame_complete_ms: 0.0,
            producer: 0,
            source_count: 0,
            contributor_count: 0,
            drawn_count: 0,
            draw_scope: 0,
            flags: 0,
            reserved: 0,
        }
    }
}

/// Terminal failure for one already-exposed producer ticket.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsplatSurfaceGpuProducerFailureV1 {
    pub struct_size: u32,
    pub version: u32,
    pub ticket: u64,
    pub camera_revision: u64,
    pub order_generation: u64,
    pub projection_generation: u64,
    pub reason: u32,
    pub producer: u32,
    pub flags: u32,
    pub reserved: u32,
}

impl Default for GsplatSurfaceGpuProducerFailureV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_GPU_PRODUCER_ABI_VERSION_V1,
            ticket: 0,
            camera_revision: 0,
            order_generation: 0,
            projection_generation: 0,
            reason: 0,
            producer: 0,
            flags: 0,
            reserved: 0,
        }
    }
}

/// Source-to-GPU exactness and adapter-admission receipt for the Surface
/// renderer's current geometry path.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GsplatSurfaceExactness {
    pub source_splat_count: u64,
    pub decoded_splat_count: u64,
    pub encoded_splat_count: u64,
    pub resident_splat_count: u64,
    pub addressable_splat_count: u64,
    pub source_sh_degree: u32,
    pub resident_sh_degree: u32,
    pub quality_flags: u32,
    pub max_storage_buffers_per_shader_stage: u32,
    pub max_storage_buffer_binding_size: u64,
}

/// Pixel-resolution receipt for the current native Surface presentation path.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GsplatSurfacePresentation {
    pub requested_width: u32,
    pub requested_height: u32,
    pub surface_width: u32,
    pub surface_height: u32,
    pub internal_render_width: u32,
    pub internal_render_height: u32,
    pub presented_width: u32,
    pub presented_height: u32,
    pub presented_camera_revision: u64,
    pub flags: u32,
    pub reserved: u32,
}

const SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1: u32 = 1;

/// Versioned read-only receipt for the camera state actually owned by the
/// shared Surface session after a render call.
///
/// The canonical matrices are derived from this f32 state and the current
/// Surface aspect. They are never copied from a camera-trace payload.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GsplatSurfaceCameraReceiptV1 {
    pub struct_size: u32,
    pub version: u32,
    pub camera_revision: u64,
    pub presented_camera_revision: u64,
    pub surface_width: u32,
    pub surface_height: u32,
    pub flags: u32,
    pub reserved: u32,
    pub position: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub vertical_fov_radians: f32,
    pub near_plane: f32,
    pub far_plane: f32,
    pub view_matrix: [f32; 16],
    pub projection_matrix: [f32; 16],
    pub view_projection_matrix: [f32; 16],
}

impl Default for GsplatSurfaceCameraReceiptV1 {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            version: SURFACE_CAMERA_RECEIPT_ABI_VERSION_V1,
            camera_revision: 0,
            presented_camera_revision: 0,
            surface_width: 0,
            surface_height: 0,
            flags: 0,
            reserved: 0,
            position: [0.0; 3],
            rotation_xyzw: [0.0; 4],
            vertical_fov_radians: 0.0,
            near_plane: 0.0,
            far_plane: 0.0,
            view_matrix: [0.0; 16],
            projection_matrix: [0.0; 16],
            view_projection_matrix: [0.0; 16],
        }
    }
}

const SURFACE_ORDER_MEASUREMENT_PREPROCESS_VALID: u32 = 1 << 0;
const SURFACE_ORDER_MEASUREMENT_RADIX_VALID: u32 = 1 << 1;
const SURFACE_ORDER_MEASUREMENT_ORDER_VALID: u32 = 1 << 2;
const SURFACE_ORDER_MEASUREMENT_TIMESTAMP_PERIOD_VALID: u32 = 1 << 3;
const SURFACE_ORDER_MEASUREMENT_BELOW_TIMESTAMP_RESOLUTION: u32 = 1 << 4;
const SURFACE_ORDER_MEASUREMENT_DROPPED_PRIOR: u32 = 1 << 5;
const SURFACE_ORDER_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW: u32 = 1 << 6;
const SURFACE_CPU_ORDER_MEASUREMENT_DROPPED_PRIOR: u32 = 1 << 0;
const SURFACE_CPU_ORDER_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW: u32 = 1 << 1;
const SURFACE_CPU_ORDER_MEASUREMENT_CONTRIBUTOR_COUNT_VALID: u32 = 1 << 2;
const SURFACE_ORDER_COUNTS_EXACT_CONTRIBUTOR_DRAW: u32 = 1 << 0;
const SURFACE_ORDER_MEASUREMENT_FAILURE_DROPPED_PRIOR: u32 = 1 << 0;
const SURFACE_ORDER_SUBMISSION_GPU_REFRESH: u32 = 1 << 0;
const SURFACE_ORDER_SUBMISSION_TICKET_ISSUED: u32 = 1 << 1;
const SURFACE_ORDER_SUBMISSION_UNSAMPLED_RING_BUSY: u32 = 1 << 2;
const SURFACE_ORDER_SUBMISSION_CPU_FRAME_COMPLETION_SAMPLE: u32 = 1 << 3;
const SURFACE_ORDER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE: u32 = 1 << 4;

const SURFACE_PROJECTED_SUBMISSION_TICKET_ISSUED: u32 = 1 << 0;
const SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_RING_BUSY: u32 = 1 << 1;
const SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE: u32 = 1 << 2;
const SURFACE_PROJECTED_MEASUREMENT_PROJECTION_REBUILT: u32 = 1 << 0;
const SURFACE_PROJECTED_MEASUREMENT_ORDER_REFRESHED: u32 = 1 << 1;
const SURFACE_PROJECTED_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW: u32 = 1 << 2;
const SURFACE_PROJECTED_MEASUREMENT_DROPPED_PRIOR: u32 = 1 << 3;
const SURFACE_PROJECTED_COUNTS_EXACT_CONTRIBUTOR_DRAW: u32 = 1 << 0;
const SURFACE_PROJECTED_FAILURE_DROPPED_PRIOR: u32 = 1 << 0;

const SURFACE_GPU_PRODUCER_SUBMISSION_TICKET_ISSUED: u32 = 1 << 0;
const SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_RING_BUSY: u32 = 1 << 1;
const SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE: u32 = 1 << 2;
const SURFACE_GPU_PRODUCER_SUBMISSION_MEASUREMENT_ENABLED: u32 = 1 << 3;
const SURFACE_GPU_PRODUCER_MEASUREMENT_ORDER_REFRESHED: u32 = 1 << 0;
const SURFACE_GPU_PRODUCER_MEASUREMENT_EXACT_CURRENT_DRAW: u32 = 1 << 1;
const SURFACE_GPU_PRODUCER_MEASUREMENT_STALE_ORDER: u32 = 1 << 2;

const SURFACE_EXACTNESS_SOURCE_MEMBERSHIP_ALL: u32 = 1 << 0;
const SURFACE_EXACTNESS_SAMPLING_DISABLED: u32 = 1 << 1;
const SURFACE_EXACTNESS_LOD_DISABLED: u32 = 1 << 2;
const SURFACE_EXACTNESS_SH_DEGREE_SOURCE: u32 = 1 << 3;
const SURFACE_EXACTNESS_PARTIAL_SCENE_NOT_PUBLISHED: u32 = 1 << 4;
const SURFACE_EXACTNESS_FULL_QUALITY_FLAGS: u32 = SURFACE_EXACTNESS_SOURCE_MEMBERSHIP_ALL
    | SURFACE_EXACTNESS_SAMPLING_DISABLED
    | SURFACE_EXACTNESS_LOD_DISABLED
    | SURFACE_EXACTNESS_SH_DEGREE_SOURCE
    | SURFACE_EXACTNESS_PARTIAL_SCENE_NOT_PUBLISHED;
const SURFACE_PRESENTATION_LAST_FRAME_PRESENTED: u32 = 1 << 0;
const SURFACE_PRESENTATION_EVER_PRESENTED: u32 = 1 << 1;
const SURFACE_PRESENTATION_DYNAMIC_RESOLUTION_DISABLED: u32 = 1 << 2;
const SURFACE_PRESENTATION_UPSCALING_DISABLED: u32 = 1 << 3;
const SURFACE_PRESENTATION_FULL_RESOLUTION: u32 = 1 << 4;
const SURFACE_CAMERA_RECEIPT_FRAME_PRESENTED: u32 = 1 << 0;
const SURFACE_CAMERA_RECEIPT_CURRENT_REVISION_PRESENTED: u32 = 1 << 1;

fn surface_order_backend_to_ffi(backend: SurfaceOrderBackend) -> u32 {
    match backend {
        SurfaceOrderBackend::Cpu => 0,
        SurfaceOrderBackend::Gpu => 1,
        SurfaceOrderBackend::Adaptive => 2,
    }
}

fn surface_order_backend_used_to_ffi(backend: SurfaceOrderBackendUsed) -> u32 {
    match backend {
        SurfaceOrderBackendUsed::Cpu => 0,
        SurfaceOrderBackendUsed::Gpu => 1,
    }
}

fn surface_projected_policy_to_ffi(policy: SurfaceProjectedDrawPolicy) -> u32 {
    match policy {
        SurfaceProjectedDrawPolicy::Candidate => 1,
        SurfaceProjectedDrawPolicy::Compact => 2,
        SurfaceProjectedDrawPolicy::Adaptive => 3,
    }
}

fn surface_projected_policy_from_ffi(value: u32) -> Option<SurfaceProjectedDrawPolicy> {
    match value {
        // Zero is the versioned ABI's legacy/default selector. Existing C
        // clients did not choose this independent policy, and the Rust
        // session's compatibility default is Adaptive.
        0 | 3 => Some(SurfaceProjectedDrawPolicy::Adaptive),
        1 => Some(SurfaceProjectedDrawPolicy::Candidate),
        2 => Some(SurfaceProjectedDrawPolicy::Compact),
        _ => None,
    }
}

fn surface_projected_execution_to_ffi(execution: SurfaceProjectedDrawExecution) -> u32 {
    match execution {
        SurfaceProjectedDrawExecution::Candidate => 1,
        SurfaceProjectedDrawExecution::Compact => 2,
    }
}

fn surface_projected_adaptive_state_to_ffi(state: SurfaceProjectedDrawAdaptiveState) -> u32 {
    match state {
        SurfaceProjectedDrawAdaptiveState::Disabled => 0,
        SurfaceProjectedDrawAdaptiveState::CandidateLearning => 1,
        SurfaceProjectedDrawAdaptiveState::CandidateStable => 2,
        SurfaceProjectedDrawAdaptiveState::CompactProbe => 3,
        SurfaceProjectedDrawAdaptiveState::CompactStable => 4,
        SurfaceProjectedDrawAdaptiveState::CandidateProbe => 5,
        SurfaceProjectedDrawAdaptiveState::Cooldown => 6,
        SurfaceProjectedDrawAdaptiveState::CandidateOnly => 7,
    }
}

#[repr(C)]
struct GsplatV1Header {
    struct_size: u32,
    version: u32,
}

fn validate_versioned_output<T>(
    output: *mut T,
    expected_version: u32,
    operation: &'static str,
) -> Result<(), i32> {
    if output.is_null() {
        return Err(ffi_error(
            ErrorCode::InvalidArgument,
            format!("{operation}: output is null"),
        ));
    }
    let header = unsafe { &*output.cast::<GsplatV1Header>() };
    let expected_size = std::mem::size_of::<T>() as u32;
    if header.version != expected_version {
        return Err(ffi_error(
            ErrorCode::InvalidArgument,
            format!(
                "{operation}: unsupported version {} (expected {})",
                header.version, expected_version
            ),
        ));
    }
    if header.struct_size < expected_size {
        return Err(ffi_error(
            ErrorCode::InvalidArgument,
            format!(
                "{operation}: struct_size {} is smaller than {}",
                header.struct_size, expected_size
            ),
        ));
    }
    Ok(())
}

fn validate_v1_output<T>(output: *mut T, operation: &'static str) -> Result<(), i32> {
    validate_versioned_output(output, SURFACE_PROJECTED_ABI_VERSION_V1, operation)
}

fn surface_projected_submission_to_ffi(
    submission: SurfaceCompatibilityProjectedSubmission,
) -> GsplatSurfaceProjectedSubmissionV1 {
    let (ticket, unsampled_reason) = match submission.measurement {
        SurfaceProjectedDrawMeasurementSubmission::NotRequested => (None, None),
        SurfaceProjectedDrawMeasurementSubmission::Issued { ticket, .. } => (Some(ticket), None),
        SurfaceProjectedDrawMeasurementSubmission::Unsampled { reason, .. } => (None, Some(reason)),
    };
    let mut flags = u32::from(ticket.is_some()) * SURFACE_PROJECTED_SUBMISSION_TICKET_ISSUED;
    flags |= u32::from(
        unsampled_reason == Some(SurfaceProjectedDrawMeasurementUnsampledReason::RingBusy),
    ) * SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_RING_BUSY;
    flags |= u32::from(
        unsampled_reason
            == Some(SurfaceProjectedDrawMeasurementUnsampledReason::SurfaceUnavailable),
    ) * SURFACE_PROJECTED_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE;
    GsplatSurfaceProjectedSubmissionV1 {
        ticket: ticket.unwrap_or(0),
        camera_revision: submission.camera_revision,
        requested_policy: surface_projected_policy_to_ffi(submission.requested_policy),
        actual_execution: surface_projected_execution_to_ffi(submission.actual_execution),
        order_backend: surface_order_backend_used_to_ffi(submission.order_backend),
        adaptive_state: surface_projected_adaptive_state_to_ffi(submission.adaptive_state),
        flags,
        ..Default::default()
    }
}

fn surface_projected_measurement_to_ffi(
    measurement: SurfaceCompatibilityProjectedSuccess,
) -> GsplatSurfaceProjectedMeasurementV1 {
    let mut flags = 0;
    flags |= u32::from(measurement.projection_rebuilt)
        * SURFACE_PROJECTED_MEASUREMENT_PROJECTION_REBUILT;
    flags |= u32::from(measurement.order_refreshed) * SURFACE_PROJECTED_MEASUREMENT_ORDER_REFRESHED;
    flags |= u32::from(measurement.exact_contributor_compaction)
        * SURFACE_PROJECTED_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW;
    flags |= u32::from(measurement.dropped_prior) * SURFACE_PROJECTED_MEASUREMENT_DROPPED_PRIOR;
    GsplatSurfaceProjectedMeasurementV1 {
        ticket: measurement.ticket,
        camera_revision: measurement.camera_revision,
        projection_generation: measurement.projection_generation,
        probe_generation: measurement.probe_generation,
        frame_complete_ms: measurement.frame_complete_ms,
        execution: surface_projected_execution_to_ffi(measurement.execution),
        order_backend: surface_order_backend_used_to_ffi(measurement.order_backend),
        flags,
        ..Default::default()
    }
}

fn surface_projected_counts(counts: SurfaceCompatibilityCounts) -> GsplatSurfaceProjectedCountsV1 {
    debug_assert_eq!(counts.family, SurfaceCompatibilityCountFamily::Projected);
    GsplatSurfaceProjectedCountsV1 {
        ticket: counts.ticket,
        camera_revision: counts.camera_revision,
        visible_count: counts.visible_count,
        contributor_count: counts.contributor_count,
        drawn_count: counts.drawn_count,
        flags: u32::from(counts.exact_contributor_compaction)
            * SURFACE_PROJECTED_COUNTS_EXACT_CONTRIBUTOR_DRAW,
        ..Default::default()
    }
}

fn surface_projected_failure_to_ffi(
    failure: SurfaceCompatibilityProjectedFailure,
) -> GsplatSurfaceProjectedFailureV1 {
    GsplatSurfaceProjectedFailureV1 {
        ticket: failure.ticket,
        camera_revision: failure.camera_revision,
        projection_generation: failure.projection_generation,
        probe_generation: failure.probe_generation,
        reason: match failure.reason {
            SurfaceProjectedDrawMeasurementFailureReason::ReadbackMap => 1,
            SurfaceProjectedDrawMeasurementFailureReason::GenerationInvalidated => 2,
            SurfaceProjectedDrawMeasurementFailureReason::InvariantViolation => 3,
        },
        execution: surface_projected_execution_to_ffi(failure.execution),
        order_backend: surface_order_backend_used_to_ffi(failure.order_backend),
        flags: u32::from(failure.dropped_prior) * SURFACE_PROJECTED_FAILURE_DROPPED_PRIOR,
        ..Default::default()
    }
}

fn surface_gpu_producer_to_ffi(producer: SurfaceGpuOrderProducer) -> u32 {
    match producer {
        SurfaceGpuOrderProducer::PostSort => 1,
        SurfaceGpuOrderProducer::Preproject => 2,
    }
}

fn surface_gpu_producer_from_ffi(value: u32) -> Option<SurfaceGpuOrderProducer> {
    match value {
        // Zero is a setter-only alias for the unchanged qualified default.
        0 | 1 => Some(SurfaceGpuOrderProducer::PostSort),
        2 => Some(SurfaceGpuOrderProducer::Preproject),
        _ => None,
    }
}

fn surface_gpu_producer_submission_to_ffi(
    submission: SurfaceCompatibilityProducerSubmission,
) -> GsplatSurfaceGpuProducerSubmissionV1 {
    let (ticket, sampled_producer, unsampled_reason) = match submission.measurement {
        SurfaceGpuProducerMeasurementSubmission::NotRequested => (None, None, None),
        SurfaceGpuProducerMeasurementSubmission::Issued { producer, ticket } => {
            (Some(ticket), Some(producer), None)
        }
        SurfaceGpuProducerMeasurementSubmission::Unsampled { producer, reason } => {
            (None, Some(producer), Some(reason))
        }
    };
    let actual_producer = submission.actual_producer.or(sampled_producer);
    let mut flags = u32::from(ticket.is_some()) * SURFACE_GPU_PRODUCER_SUBMISSION_TICKET_ISSUED;
    flags |=
        u32::from(unsampled_reason == Some(SurfaceGpuProducerMeasurementUnsampledReason::RingBusy))
            * SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_RING_BUSY;
    flags |= u32::from(
        unsampled_reason == Some(SurfaceGpuProducerMeasurementUnsampledReason::SurfaceUnavailable),
    ) * SURFACE_GPU_PRODUCER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE;
    flags |= u32::from(submission.measurement_enabled)
        * SURFACE_GPU_PRODUCER_SUBMISSION_MEASUREMENT_ENABLED;
    GsplatSurfaceGpuProducerSubmissionV1 {
        ticket: ticket.unwrap_or(0),
        camera_revision: submission.camera_revision,
        requested_producer: surface_gpu_producer_to_ffi(submission.requested_producer),
        actual_producer: actual_producer.map_or(0, surface_gpu_producer_to_ffi),
        order_backend: surface_order_backend_used_to_ffi(submission.order_backend),
        projected_execution: surface_projected_execution_to_ffi(submission.projected_execution),
        flags,
        ..Default::default()
    }
}

fn surface_gpu_producer_measurement_to_ffi(
    measurement: SurfaceGpuProducerMeasurement,
) -> GsplatSurfaceGpuProducerMeasurementV1 {
    let mut flags =
        u32::from(measurement.order_refreshed) * SURFACE_GPU_PRODUCER_MEASUREMENT_ORDER_REFRESHED;
    flags |= u32::from(measurement.exact_current_contributor_draw())
        * SURFACE_GPU_PRODUCER_MEASUREMENT_EXACT_CURRENT_DRAW;
    flags |= u32::from(measurement.stale_order()) * SURFACE_GPU_PRODUCER_MEASUREMENT_STALE_ORDER;
    GsplatSurfaceGpuProducerMeasurementV1 {
        ticket: measurement.ticket,
        camera_revision: measurement.camera_revision,
        order_generation: measurement.order_generation,
        projection_generation: measurement.projection_generation,
        frame_complete_ms: measurement.frame_complete_ms,
        producer: surface_gpu_producer_to_ffi(measurement.producer),
        source_count: measurement.source_count,
        contributor_count: measurement.contributor_count,
        drawn_count: measurement.drawn_count,
        draw_scope: match measurement.draw_scope {
            SurfaceGpuProducerDrawScope::ExactCurrentContributors => 1,
            SurfaceGpuProducerDrawScope::StaleOrderCandidates => 2,
        },
        flags,
        ..Default::default()
    }
}

fn surface_gpu_producer_failure_to_ffi(
    failure: SurfaceGpuProducerMeasurementFailure,
) -> GsplatSurfaceGpuProducerFailureV1 {
    GsplatSurfaceGpuProducerFailureV1 {
        ticket: failure.ticket,
        camera_revision: failure.camera_revision,
        order_generation: failure.order_generation,
        projection_generation: failure.projection_generation,
        reason: match failure.reason {
            SurfaceGpuProducerMeasurementFailureReason::ReadbackMap => 1,
            SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated => 2,
            SurfaceGpuProducerMeasurementFailureReason::InvariantViolation => 3,
        },
        producer: surface_gpu_producer_to_ffi(failure.producer),
        ..Default::default()
    }
}

fn surface_adaptive_state_to_ffi(state: SurfaceAdaptiveState) -> u32 {
    match state {
        SurfaceAdaptiveState::Disabled => 0,
        SurfaceAdaptiveState::CpuLearning => 1,
        SurfaceAdaptiveState::CpuStable => 2,
        SurfaceAdaptiveState::GpuProbe => 3,
        SurfaceAdaptiveState::GpuStable => 4,
        SurfaceAdaptiveState::CpuProbe => 5,
        SurfaceAdaptiveState::Cooldown => 6,
    }
}

fn surface_adaptive_gpu_failure_flags(failure: Option<SurfaceAdaptiveGpuFailureReason>) -> u32 {
    let Some(failure) = failure else {
        return 0;
    };
    let reason = match failure {
        SurfaceAdaptiveGpuFailureReason::Unsupported => 1,
        SurfaceAdaptiveGpuFailureReason::Initialization => 2,
        SurfaceAdaptiveGpuFailureReason::OutOfMemory => 3,
        SurfaceAdaptiveGpuFailureReason::Validation => 4,
    };
    (1 << 15) | (reason << 16)
}

fn surface_order_measurement_to_ffi(
    measurement: SurfaceCompatibilityOrderGpuSuccess,
) -> GsplatSurfaceOrderMeasurement {
    let context = SurfaceOrderMeasurementContext::from(measurement.issue);
    let mut flags = 0_u32;
    let gpu_preprocess_ms = measurement.gpu_preprocess_ms.unwrap_or(0.0);
    flags |= u32::from(measurement.gpu_preprocess_ms.is_some())
        * SURFACE_ORDER_MEASUREMENT_PREPROCESS_VALID;
    let gpu_radix_ms = measurement.gpu_radix_ms.unwrap_or(0.0);
    flags |= u32::from(measurement.gpu_radix_ms.is_some()) * SURFACE_ORDER_MEASUREMENT_RADIX_VALID;
    let gpu_order_ms = measurement.gpu_order_ms.unwrap_or(0.0);
    flags |= u32::from(measurement.gpu_order_ms.is_some()) * SURFACE_ORDER_MEASUREMENT_ORDER_VALID;
    let timestamp_period_ns = measurement.timestamp_period_ns.unwrap_or(0.0);
    flags |= u32::from(measurement.timestamp_period_ns.is_some())
        * SURFACE_ORDER_MEASUREMENT_TIMESTAMP_PERIOD_VALID;
    flags |= u32::from(measurement.below_timestamp_resolution)
        * SURFACE_ORDER_MEASUREMENT_BELOW_TIMESTAMP_RESOLUTION;
    flags |= u32::from(measurement.exact_contributor_compaction)
        * SURFACE_ORDER_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW;
    flags |= u32::from(measurement.dropped_prior) * SURFACE_ORDER_MEASUREMENT_DROPPED_PRIOR;

    GsplatSurfaceOrderMeasurement {
        ticket: measurement.ticket,
        camera_revision: measurement.camera_revision,
        gpu_preprocess_ms,
        gpu_radix_ms,
        gpu_order_ms,
        gpu_complete_ms: measurement.gpu_complete_ms,
        timestamp_period_ns,
        visible_count: measurement.visible_count,
        drawn_count: measurement.drawn_count,
        timing_source: match measurement.timing_source {
            SurfaceTimingSource::TimestampQuery => 1,
            SurfaceTimingSource::CompletionOnly => 2,
        },
        requested_backend: surface_order_backend_to_ffi(context.requested_backend),
        actual_backend: surface_order_backend_used_to_ffi(context.actual_backend),
        adaptive_state: surface_adaptive_state_to_ffi(context.adaptive_state),
        flags,
    }
}

fn surface_cpu_order_measurement_to_ffi(
    measurement: SurfaceCompatibilityOrderCpuSuccess,
) -> GsplatSurfaceCpuOrderMeasurement {
    let context = SurfaceOrderMeasurementContext::from(measurement.issue);
    let mut flags = SURFACE_CPU_ORDER_MEASUREMENT_CONTRIBUTOR_COUNT_VALID;
    flags |= u32::from(measurement.exact_contributor_compaction)
        * SURFACE_CPU_ORDER_MEASUREMENT_EXACT_CONTRIBUTOR_DRAW;
    flags |= u32::from(measurement.dropped_prior) * SURFACE_CPU_ORDER_MEASUREMENT_DROPPED_PRIOR;
    GsplatSurfaceCpuOrderMeasurement {
        ticket: measurement.ticket,
        camera_revision: measurement.camera_revision,
        preprocess_ms: measurement.preprocess_ms,
        sort_ms: measurement.sort_ms,
        frame_complete_ms: measurement.frame_complete_ms,
        requested_backend: surface_order_backend_to_ffi(context.requested_backend),
        actual_backend: surface_order_backend_used_to_ffi(context.actual_backend),
        adaptive_state: surface_adaptive_state_to_ffi(context.adaptive_state),
        flags,
        // ABI-preserving additive payload: C lives in the existing reserved
        // word. V/D remain the same-ticket frame stats and must be joined by
        // both ticket and camera_revision.
        reserved: measurement.contributor_count,
    }
}

fn surface_order_counts(counts: SurfaceCompatibilityCounts) -> GsplatSurfaceOrderCounts {
    debug_assert_eq!(counts.family, SurfaceCompatibilityCountFamily::Order);
    GsplatSurfaceOrderCounts {
        ticket: counts.ticket,
        camera_revision: counts.camera_revision,
        visible_count: counts.visible_count,
        contributor_count: counts.contributor_count,
        drawn_count: counts.drawn_count,
        flags: u32::from(counts.exact_contributor_compaction)
            * SURFACE_ORDER_COUNTS_EXACT_CONTRIBUTOR_DRAW,
    }
}

fn ready_compatibility_counts(
    take: SurfaceCompatibilityCountsTake,
) -> Option<SurfaceCompatibilityCounts> {
    match take {
        SurfaceCompatibilityCountsTake::Ready(counts) => Some(counts),
        SurfaceCompatibilityCountsTake::Unavailable(_) => None,
    }
}

fn surface_order_measurement_failure_to_ffi(
    failure: SurfaceCompatibilityOrderFailure,
) -> GsplatSurfaceOrderMeasurementFailure {
    let context = SurfaceOrderMeasurementContext::from(failure.issue);
    GsplatSurfaceOrderMeasurementFailure {
        ticket: failure.ticket,
        camera_revision: failure.camera_revision,
        reason: match failure.reason {
            SurfaceOrderMeasurementFailureReason::ReadbackMap => 1,
            SurfaceOrderMeasurementFailureReason::GenerationInvalidated => 2,
        },
        requested_backend: surface_order_backend_to_ffi(context.requested_backend),
        actual_backend: surface_order_backend_used_to_ffi(context.actual_backend),
        adaptive_state: surface_adaptive_state_to_ffi(context.adaptive_state),
        flags: u32::from(failure.dropped_prior) * SURFACE_ORDER_MEASUREMENT_FAILURE_DROPPED_PRIOR,
        reserved: 0,
    }
}

fn surface_order_submission_to_ffi(
    submission: SurfaceCompatibilityOrderSubmission,
) -> GsplatSurfaceOrderSubmission {
    let (measurement_backend, ticket, unsampled_reason) = match submission.measurement {
        SurfaceOrderMeasurementSubmission::NotRequested => (None, None, None),
        SurfaceOrderMeasurementSubmission::Issued { backend, ticket } => {
            (Some(backend), Some(ticket), None)
        }
        SurfaceOrderMeasurementSubmission::Unsampled { backend, reason } => {
            (Some(backend), None, Some(reason))
        }
    };
    let mut flags = match measurement_backend {
        Some(SurfaceOrderBackendUsed::Gpu) => SURFACE_ORDER_SUBMISSION_GPU_REFRESH,
        Some(SurfaceOrderBackendUsed::Cpu) => SURFACE_ORDER_SUBMISSION_CPU_FRAME_COMPLETION_SAMPLE,
        None => 0,
    };
    flags |= u32::from(ticket.is_some()) * SURFACE_ORDER_SUBMISSION_TICKET_ISSUED;
    flags |= u32::from(unsampled_reason == Some(SurfaceOrderMeasurementUnsampledReason::RingBusy))
        * SURFACE_ORDER_SUBMISSION_UNSAMPLED_RING_BUSY;
    flags |= u32::from(
        unsampled_reason == Some(SurfaceOrderMeasurementUnsampledReason::SurfaceUnavailable),
    ) * SURFACE_ORDER_SUBMISSION_UNSAMPLED_SURFACE_UNAVAILABLE;
    GsplatSurfaceOrderSubmission {
        ticket: ticket.unwrap_or(0),
        camera_revision: submission.camera_revision,
        requested_backend: surface_order_backend_to_ffi(submission.requested_backend),
        actual_backend: surface_order_backend_used_to_ffi(submission.actual_backend),
        adaptive_state: surface_adaptive_state_to_ffi(submission.adaptive_state),
        flags,
    }
}

impl From<SurfaceFrameOutput> for GsplatSurfaceSortStats {
    fn from(output: SurfaceFrameOutput) -> Self {
        let mut flags = 0_u32;
        flags |= u32::from(output.sort_refreshed);
        flags |= u32::from(output.order_uploaded) << 1;
        flags |= u32::from(output.async_sort_scheduled) << 2;
        flags |= u32::from(output.async_sort_scheduled_revision.is_some()) << 3;
        flags |= u32::from(output.async_sort_completed_revision.is_some()) << 4;
        flags |= u32::from(output.async_sort_result_applied) << 5;
        flags |= u32::from(output.stale_async_sort_dropped) << 6;
        flags |= u32::from(output.sync_sort_fallback) << 7;
        flags |= u32::from(output.async_sort_revision_lag.is_some()) << 8;
        flags |= match output.order_backend {
            SurfaceOrderBackendUsed::Cpu => 0,
            SurfaceOrderBackendUsed::Gpu => 1,
        } << 9;
        flags |= u32::from(output.gpu_sort_fallback) << 11;
        flags |= match output.adaptive_state {
            SurfaceAdaptiveState::Disabled => 0,
            SurfaceAdaptiveState::CpuLearning => 1,
            SurfaceAdaptiveState::CpuStable => 2,
            SurfaceAdaptiveState::GpuProbe => 3,
            SurfaceAdaptiveState::GpuStable => 4,
            SurfaceAdaptiveState::CpuProbe => 5,
            SurfaceAdaptiveState::Cooldown => 6,
        } << 12;
        flags |= surface_adaptive_gpu_failure_flags(output.adaptive_gpu_failure);
        flags |= u32::from(
            output.order_backend == SurfaceOrderBackendUsed::Gpu
                && output.submitted_measurement_ticket.is_some(),
        ) << 19;
        Self {
            camera_revision: output.camera_revision,
            applied_order_revision: output.applied_order_revision,
            scheduled_revision: output.async_sort_scheduled_revision.unwrap_or(0),
            completed_revision: output.async_sort_completed_revision.unwrap_or(0),
            presented_order_revision_lag: output.presented_order_revision_lag,
            observed_result_revision_lag: output.async_sort_revision_lag.unwrap_or(0),
            flags,
        }
    }
}

impl From<FrameStats> for GsplatStats {
    fn from(stats: FrameStats) -> Self {
        Self {
            frame_ms: stats.frame_ms,
            preprocess_ms: stats.preprocess_ms,
            sort_ms: stats.sort_ms,
            raster_ms: stats.raster_ms,
            visible_count: stats.visible_count,
            drawn_count: stats.drawn_count,
        }
    }
}

fn copy_legacy_surface_stats(
    stats: Option<FrameStats>,
    out_stats: &mut GsplatStats,
) -> Result<(), ErrorCode> {
    // SurfaceRenderSession exposes Some only for synchronous current counts or
    // a READY receipt joined to the last presented ticket and full identity.
    let stats = stats.ok_or(ErrorCode::NotFound)?;
    *out_stats = stats.into();
    Ok(())
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsplatCamera {
    pub position: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub vertical_fov_radians: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl Default for GsplatCamera {
    fn default() -> Self {
        let camera = Camera::default();
        Self {
            position: [
                camera.pose.position.x,
                camera.pose.position.y,
                camera.pose.position.z,
            ],
            rotation_xyzw: camera.pose.rotation_xyzw,
            vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
            near_plane: camera.intrinsics.near_plane,
            far_plane: camera.intrinsics.far_plane,
        }
    }
}

impl From<GsplatCamera> for Camera {
    fn from(value: GsplatCamera) -> Self {
        Self {
            pose: CameraPose {
                position: Vec3f::new(value.position[0], value.position[1], value.position[2]),
                rotation_xyzw: value.rotation_xyzw,
            },
            intrinsics: CameraIntrinsics {
                vertical_fov_radians: value.vertical_fov_radians,
                near_plane: value.near_plane,
                far_plane: value.far_plane,
                // The stable v0.1 C camera input predates calibrated non-square
                // pixels and therefore preserves its square-pixel projection.
                focal_length_x_over_y: 1.0,
            },
        }
    }
}

pub struct GsplatContext {
    renderer: Renderer,
    camera: Camera,
}

pub struct GsplatSurfaceRenderer {
    session: SurfaceRenderSession,
    exactness: GsplatSurfaceExactness,
    requested_surface_width: u32,
    requested_surface_height: u32,
    last_presented_camera_revision: u64,
    last_frame_presented: bool,
    ever_presented: bool,
    camera_control: SurfaceCameraControl,
    render_error_logged: bool,
    last_sort_stats: GsplatSurfaceSortStats,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    benchmark_camera_trace: Option<BenchmarkCameraTraceCache>,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceOrderMeasurementContext {
    requested_backend: SurfaceOrderBackend,
    actual_backend: SurfaceOrderBackendUsed,
    adaptive_state: SurfaceAdaptiveState,
}

impl From<SurfaceCompatibilityOrderIssueContext> for SurfaceOrderMeasurementContext {
    fn from(context: SurfaceCompatibilityOrderIssueContext) -> Self {
        Self {
            requested_backend: context.requested_backend,
            actual_backend: context.actual_backend,
            adaptive_state: context.adaptive_state,
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
struct BenchmarkCameraTraceCache {
    path: String,
    trace: CameraTrace,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceCameraControl {
    target: Vec3f,
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

fn log_surface_error(message: &str) {
    eprintln!("gsplat-ffi-c: {message}");
}

fn static_cstr(bytes: &'static [u8]) -> *const c_char {
    bytes.as_ptr() as *const c_char
}

thread_local! {
    static LAST_ERROR_MESSAGE: RefCell<CString> =
        RefCell::new(CString::new("ok").expect("static ok string contains no nul"));
}

fn make_cstring(message: impl Into<String>) -> CString {
    let bytes: Vec<u8> = message
        .into()
        .into_bytes()
        .into_iter()
        .filter(|byte| *byte != 0)
        .collect();
    CString::new(bytes).unwrap_or_else(|_| CString::new("internal error").unwrap())
}

fn set_last_error_message(message: impl Into<String>) {
    LAST_ERROR_MESSAGE.with(|cell| {
        *cell.borrow_mut() = make_cstring(message);
    });
}

fn ffi_ok() -> i32 {
    set_last_error_message("ok");
    ErrorCode::Ok.as_i32()
}

fn ffi_error(code: ErrorCode, message: impl Into<String>) -> i32 {
    set_last_error_message(message);
    code.as_i32()
}

fn ffi_error_display(code: ErrorCode, operation: &str, error: impl Display) -> i32 {
    ffi_error(code, format!("{operation}: {error}"))
}

#[derive(Debug)]
enum PathSceneLoadError {
    Ply(PlyLoadError),
    Renderer(RendererError),
    SummaryChanged {
        expected: PlySceneSummary,
        decoded: PlySceneSummary,
    },
}

impl PathSceneLoadError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::Ply(error) => error.code(),
            Self::Renderer(error) => error.code(),
            Self::SummaryChanged { .. } => ErrorCode::ParseFailed,
        }
    }
}

impl std::fmt::Display for PathSceneLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ply(error) => error.fmt(formatter),
            Self::Renderer(error) => error.fmt(formatter),
            Self::SummaryChanged { expected, decoded } => write!(
                formatter,
                "PLY changed while loading: expected {expected:?}, decoded {decoded:?}"
            ),
        }
    }
}

fn load_ply_path_into_renderer(
    path: &Path,
    renderer: &mut Renderer,
) -> Result<PlySceneSummary, PathSceneLoadError> {
    if renderer.geometry_path() != GeometryPath::PackedAtlas {
        let loaded = load_ply(path).map_err(PathSceneLoadError::Ply)?;
        let summary = loaded.summary;
        renderer
            .load_scene(loaded.scene)
            .map_err(PathSceneLoadError::Renderer)?;
        return Ok(summary);
    }

    let expected = load_ply_summary(path).map_err(PathSceneLoadError::Ply)?;
    let mut builder = ResidentSceneBuilder::new(expected.gaussians, expected.sh_degree)
        .map_err(RendererError::from)
        .map_err(PathSceneLoadError::Renderer)?;
    let mut builder_error: Option<ResidentSceneError> = None;
    let decoded = visit_ply_splats(path, |splat| {
        if builder_error.is_none() {
            builder_error = builder.push(resident_source_splat(splat)).err();
        }
    })
    .map_err(PathSceneLoadError::Ply)?;
    if let Some(error) = builder_error {
        return Err(PathSceneLoadError::Renderer(RendererError::from(error)));
    }
    if decoded != expected {
        return Err(PathSceneLoadError::SummaryChanged { expected, decoded });
    }
    let resident = builder
        .finish()
        .map_err(RendererError::from)
        .map_err(PathSceneLoadError::Renderer)?;
    renderer
        .load_resident_scene(resident)
        .map_err(PathSceneLoadError::Renderer)?;
    Ok(decoded)
}

fn resident_source_splat(splat: &DecodedPlySplat) -> ResidentSourceSplat {
    ResidentSourceSplat {
        position: splat.position_ruf,
        opacity_logit: splat.opacity_logit,
        log_scale: splat.log_scale_xyz,
        rotation_xyzw: splat.rotation_xyzw,
        color_dc: splat.color_dc,
        sh_rest: splat.sh_rest,
        sh_len: splat.sh_rest_len,
        sh_degree: splat.sh_degree,
    }
}

fn record_ffi_panic(operation: &str) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        set_last_error_message(format!("{operation}: panic caught at C ABI boundary"));
    }));
}

fn ffi_catch_i32(operation: &str, body: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            record_ffi_panic(operation);
            ErrorCode::Internal.as_i32()
        }
    }
}

fn ffi_catch_void(operation: &str, body: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(body)).is_err() {
        record_ffi_panic(operation);
    }
}

fn ffi_catch_value<T>(operation: &str, fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            record_ffi_panic(operation);
            fallback
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_version_major() -> u32 {
    ffi_catch_value("gsplat_version_major", 0, || GSPLAT_API_VERSION_MAJOR)
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_version_minor() -> u32 {
    ffi_catch_value("gsplat_version_minor", 0, || GSPLAT_API_VERSION_MINOR)
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_error_message(code: i32) -> *const c_char {
    ffi_catch_value(
        "gsplat_error_message",
        static_cstr(b"internal error\0"),
        || match code {
            0 => static_cstr(b"ok\0"),
            1 => static_cstr(b"invalid argument\0"),
            2 => static_cstr(b"not found\0"),
            3 => static_cstr(b"parse failed\0"),
            4 => static_cstr(b"unsupported\0"),
            5 => static_cstr(b"scene not loaded\0"),
            100 => static_cstr(b"internal error\0"),
            _ => static_cstr(b"unknown error\0"),
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_last_error_message() -> *const c_char {
    ffi_catch_value(
        "gsplat_last_error_message",
        static_cstr(b"internal error\0"),
        || LAST_ERROR_MESSAGE.with(|cell| cell.borrow().as_ptr()),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_config_default() -> GsplatConfig {
    ffi_catch_value(
        "gsplat_config_default",
        GsplatConfig {
            width: 0,
            height: 0,
            mode: RenderMode::SortedAlpha as u32,
        },
        GsplatConfig::default,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_camera_default() -> GsplatCamera {
    ffi_catch_value(
        "gsplat_camera_default",
        GsplatCamera {
            position: [0.0; 3],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            vertical_fov_radians: 0.0,
            near_plane: 0.0,
            far_plane: 0.0,
        },
        GsplatCamera::default,
    )
}

/// Create a renderer context.
///
/// # Safety
///
/// `out_ctx` must be valid for one pointer write. On success, the returned
/// handle must be released exactly once with `gsplat_context_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_create(
    config: GsplatConfig,
    out_ctx: *mut *mut GsplatContext,
) -> i32 {
    ffi_catch_i32("gsplat_context_create", || {
        if out_ctx.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_create: out_ctx is null",
            );
        }

        unsafe {
            *out_ctx = std::ptr::null_mut();
        }

        if config.mode != RenderMode::SortedAlpha as u32 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_create: unsupported render mode",
            );
        }

        let renderer_config = RendererConfig {
            width: config.width,
            height: config.height,
            mode: RenderMode::SortedAlpha,
        };

        let renderer = match Renderer::with_config(renderer_config) {
            Ok(renderer) => renderer,
            Err(err) => return ffi_error_display(err.code(), "gsplat_context_create", err),
        };

        let context = Box::new(GsplatContext {
            renderer,
            camera: Camera::default(),
        });

        unsafe {
            *out_ctx = Box::into_raw(context);
        }

        ffi_ok()
    })
}

/// Destroy a renderer context.
///
/// # Safety
///
/// `ctx` may be null. Non-null values must be handles returned by
/// `gsplat_context_create` that have not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_destroy(ctx: *mut GsplatContext) {
    ffi_catch_void("gsplat_context_destroy", || {
        if ctx.is_null() {
            return;
        }

        unsafe {
            drop(Box::from_raw(ctx));
        }
    });
}

/// Replace the current camera for a context.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_set_camera(
    ctx: *mut GsplatContext,
    camera: GsplatCamera,
) -> i32 {
    ffi_catch_i32("gsplat_context_set_camera", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_set_camera: ctx is null",
                );
            }
        };

        let camera: Camera = camera.into();
        if camera.validate().is_err() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_set_camera: invalid camera",
            );
        }

        ctx.camera = camera;
        ffi_ok()
    })
}

/// Set the context camera to an automatically framed view of the loaded scene.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_set_auto_camera(ctx: *mut GsplatContext) -> i32 {
    ffi_catch_i32("gsplat_context_set_auto_camera", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_set_auto_camera: ctx is null",
                );
            }
        };

        match auto_camera(&ctx.renderer) {
            Ok(camera) => {
                ctx.camera = camera;
                ffi_ok()
            }
            Err(code) => ffi_error(code, "gsplat_context_set_auto_camera: scene is not loaded"),
        }
    })
}

/// Load a scene from a filesystem path.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
/// `path` must be a non-null, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_load_scene_path(
    ctx: *mut GsplatContext,
    path: *const c_char,
) -> i32 {
    ffi_catch_i32("gsplat_context_load_scene_path", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_load_scene_path: ctx is null",
                );
            }
        };

        if path.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_load_scene_path: path is null",
            );
        }

        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(path) => path,
            Err(_) => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_load_scene_path: path is not valid UTF-8",
                );
            }
        };

        match load_ply_path_into_renderer(Path::new(path_str), &mut ctx.renderer) {
            Ok(_) => ffi_ok(),
            Err(err) => ffi_error_display(err.code(), "gsplat_context_load_scene_path", err),
        }
    })
}

/// Render one offscreen frame for a context.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_render_frame(ctx: *mut GsplatContext) -> i32 {
    ffi_catch_i32("gsplat_context_render_frame", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_render_frame: ctx is null",
                );
            }
        };

        match ctx.renderer.render_frame(&ctx.camera) {
            Ok(_) => ffi_ok(),
            Err(err) => ffi_error_display(err.code(), "gsplat_context_render_frame", err),
        }
    })
}

/// Copy the last frame stats for a context.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
/// `out_stats` must be valid for one `GsplatStats` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_get_stats(
    ctx: *const GsplatContext,
    out_stats: *mut GsplatStats,
) -> i32 {
    ffi_catch_i32("gsplat_context_get_stats", || {
        let ctx = match unsafe { ctx.as_ref() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_get_stats: ctx is null",
                );
            }
        };

        if out_stats.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_get_stats: out_stats is null",
            );
        }

        unsafe {
            *out_stats = ctx.renderer.last_stats().into();
        }

        ffi_ok()
    })
}

/// Create an Android Surface renderer from an `ANativeWindow`.
///
/// # Safety
///
/// `native_window` must be a valid `ANativeWindow` for the lifetime required
/// by the created Surface renderer. `path` must be a non-null, NUL-terminated
/// C string. `out_renderer` must be valid for one pointer write. On success,
/// the returned handle must be destroyed with `gsplat_surface_renderer_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_android(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_android_surface_renderer(
            native_window,
            path,
            width,
            height,
            1,
            out_renderer,
            "gsplat_surface_renderer_create_android",
        )
    }
}

/// Create an Android Surface renderer with a preselected experimental geometry path.
///
/// # Safety
///
/// The pointer requirements match [`gsplat_surface_renderer_create_android`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_android_with_geometry_path(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_android_surface_renderer(
            native_window,
            path,
            width,
            height,
            geometry_path,
            out_renderer,
            "gsplat_surface_renderer_create_android_with_geometry_path",
        )
    }
}

unsafe fn create_android_surface_renderer(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
    operation: &'static str,
) -> i32 {
    ffi_catch_i32(operation, || {
        if native_window.is_null() || path.is_null() || out_renderer.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                format!("{operation}: native_window, path, or out_renderer is null"),
            );
        }

        let geometry_path = match geometry_path_from_ffi(geometry_path) {
            Some(path) => path,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: unsupported geometry path"),
                );
            }
        };

        unsafe {
            *out_renderer = std::ptr::null_mut();
        }

        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(path) => path,
            Err(_) => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: path is not valid UTF-8"),
                );
            }
        };

        let mut renderer = match Renderer::with_config_for_surface(RendererConfig {
            width,
            height,
            mode: RenderMode::SortedAlpha,
        }) {
            Ok(renderer) => renderer,
            Err(err) => {
                return ffi_error_display(err.code(), operation, err);
            }
        };
        renderer.set_geometry_path(geometry_path);

        if let Err(err) = load_ply_path_into_renderer(Path::new(path_str), &mut renderer) {
            return ffi_error_display(err.code(), operation, err);
        };

        let window = match NonNull::new(native_window) {
            Some(window) => window,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: native_window is null"),
                );
            }
        };
        let raw_display_handle =
            wgpu::rwh::RawDisplayHandle::Android(wgpu::rwh::AndroidDisplayHandle::new());
        let raw_window_handle =
            wgpu::rwh::RawWindowHandle::AndroidNdk(wgpu::rwh::AndroidNdkWindowHandle::new(window));

        create_surface_renderer_from_raw_handles(
            renderer,
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            out_renderer,
        )
    })
}

/// Create a UIKit Surface renderer from a view backed by `CAMetalLayer`.
///
/// # Safety
///
/// `ui_view` must be a valid UIKit view backed by `CAMetalLayer`.
/// `ui_view_controller` may be null. `path` must be a non-null,
/// NUL-terminated C string. `out_renderer` must be valid for one pointer
/// write. On success, the returned handle must be destroyed with
/// `gsplat_surface_renderer_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_uikit(
    ui_view: *mut c_void,
    ui_view_controller: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_uikit_surface_renderer(
            (ui_view, ui_view_controller),
            path,
            width,
            height,
            1,
            out_renderer,
            "gsplat_surface_renderer_create_uikit",
        )
    }
}

/// Create a UIKit Surface renderer with a preselected experimental geometry path.
///
/// # Safety
///
/// The pointer requirements match [`gsplat_surface_renderer_create_uikit`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_uikit_with_geometry_path(
    ui_view: *mut c_void,
    ui_view_controller: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_uikit_surface_renderer(
            (ui_view, ui_view_controller),
            path,
            width,
            height,
            geometry_path,
            out_renderer,
            "gsplat_surface_renderer_create_uikit_with_geometry_path",
        )
    }
}

unsafe fn create_uikit_surface_renderer(
    ui_target: (*mut c_void, *mut c_void),
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
    operation: &'static str,
) -> i32 {
    ffi_catch_i32(operation, || {
        let (ui_view, ui_view_controller) = ui_target;
        if ui_view.is_null() || path.is_null() || out_renderer.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                format!("{operation}: ui_view, path, or out_renderer is null"),
            );
        }

        let geometry_path = match geometry_path_from_ffi(geometry_path) {
            Some(path) => path,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: unsupported geometry path"),
                );
            }
        };

        unsafe {
            *out_renderer = std::ptr::null_mut();
        }

        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(path) => path,
            Err(_) => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: path is not valid UTF-8"),
                );
            }
        };

        let mut renderer = match Renderer::with_config_for_surface(RendererConfig {
            width,
            height,
            mode: RenderMode::SortedAlpha,
        }) {
            Ok(renderer) => renderer,
            Err(err) => {
                return ffi_error_display(err.code(), operation, err);
            }
        };
        renderer.set_geometry_path(geometry_path);

        if let Err(err) = load_ply_path_into_renderer(Path::new(path_str), &mut renderer) {
            return ffi_error_display(err.code(), operation, err);
        };

        let view = match NonNull::new(ui_view) {
            Some(view) => view,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: ui_view is null"),
                );
            }
        };
        let mut window_handle = wgpu::rwh::UiKitWindowHandle::new(view);
        window_handle.ui_view_controller = NonNull::new(ui_view_controller);
        let raw_display_handle =
            wgpu::rwh::RawDisplayHandle::UiKit(wgpu::rwh::UiKitDisplayHandle::new());
        let raw_window_handle = wgpu::rwh::RawWindowHandle::UiKit(window_handle);

        create_surface_renderer_from_raw_handles(
            renderer,
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            out_renderer,
        )
    })
}

fn surface_exactness_receipt(
    session: &SurfaceRenderSession,
) -> Result<GsplatSurfaceExactness, String> {
    let renderer = session.renderer();
    let source_count = renderer
        .scene_len()
        .ok_or_else(|| "surface exactness source count is unavailable".to_owned())?;
    let source_sh_degree = renderer
        .scene_sh_degree()
        .ok_or_else(|| "surface exactness source SH degree is unavailable".to_owned())?;
    let addressable_count = session.addressable_splat_count();

    let (decoded_count, encoded_count, resident_count, resident_sh_degree, quality_flags) =
        match renderer.geometry_path() {
            GeometryPath::PackedAtlas => {
                let resident = renderer.resident_scene().ok_or_else(|| {
                    "packed Surface exactness resident scene is unavailable".to_owned()
                })?;
                (
                    resident.report.encoded_count,
                    resident.report.encoded_count,
                    resident.len(),
                    resident.sh_degree,
                    SURFACE_EXACTNESS_FULL_QUALITY_FLAGS,
                )
            }
            GeometryPath::SortedIndexDirect => (
                source_count,
                source_count,
                source_count,
                source_sh_degree,
                SURFACE_EXACTNESS_FULL_QUALITY_FLAGS,
            ),
            // Paged is intentionally diagnostic and does not claim complete
            // residency, source SH, or a non-partial publication contract.
            GeometryPath::PagedActiveAtlas => (
                source_count,
                source_count,
                0,
                0,
                SURFACE_EXACTNESS_SAMPLING_DISABLED,
            ),
        };

    if quality_flags == SURFACE_EXACTNESS_FULL_QUALITY_FLAGS
        && (decoded_count != source_count
            || encoded_count != source_count
            || resident_count != source_count
            || addressable_count != source_count
            || resident_sh_degree != source_sh_degree)
    {
        return Err(format!(
            "full-quality Surface exactness mismatch: source={source_count} decoded={decoded_count} encoded={encoded_count} resident={resident_count} addressable={addressable_count} source_sh={source_sh_degree} resident_sh={resident_sh_degree}"
        ));
    }

    let count = |value: usize| {
        u64::try_from(value).map_err(|_| "surface exactness count exceeds u64".to_owned())
    };
    Ok(GsplatSurfaceExactness {
        source_splat_count: count(source_count)?,
        decoded_splat_count: count(decoded_count)?,
        encoded_splat_count: count(encoded_count)?,
        resident_splat_count: count(resident_count)?,
        addressable_splat_count: count(addressable_count)?,
        source_sh_degree: u32::from(source_sh_degree),
        resident_sh_degree: u32::from(resident_sh_degree),
        quality_flags,
        max_storage_buffers_per_shader_stage: session
            .adapter_max_storage_buffers_per_shader_stage(),
        max_storage_buffer_binding_size: session.adapter_max_storage_buffer_binding_size(),
    })
}

fn update_surface_exactness_for_geometry_path(
    exactness: &mut GsplatSurfaceExactness,
    geometry_path: GeometryPath,
) {
    match geometry_path {
        GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => {
            exactness.decoded_splat_count = exactness.source_splat_count;
            exactness.encoded_splat_count = exactness.source_splat_count;
            exactness.resident_splat_count = exactness.source_splat_count;
            exactness.addressable_splat_count = exactness.source_splat_count;
            exactness.resident_sh_degree = exactness.source_sh_degree;
            exactness.quality_flags = SURFACE_EXACTNESS_FULL_QUALITY_FLAGS;
        }
        // Paged remains a diagnostic path. Do not publish residency,
        // addressability, source-SH, or complete-scene guarantees that its
        // active atlas does not establish.
        GeometryPath::PagedActiveAtlas => {
            exactness.decoded_splat_count = exactness.source_splat_count;
            exactness.encoded_splat_count = exactness.source_splat_count;
            exactness.resident_splat_count = 0;
            exactness.addressable_splat_count = 0;
            exactness.resident_sh_degree = 0;
            exactness.quality_flags = SURFACE_EXACTNESS_SAMPLING_DISABLED;
        }
    }
}

fn surface_presentation_receipt(renderer: &GsplatSurfaceRenderer) -> GsplatSurfacePresentation {
    let (surface_width, surface_height) = renderer.session.surface_size();
    let (internal_render_width, internal_render_height) = renderer.session.internal_render_size();
    let presented_size = renderer.session.last_presented_size();
    let (presented_width, presented_height) = presented_size.unwrap_or((0, 0));
    let full_resolution = renderer.last_frame_presented
        && renderer.requested_surface_width == surface_width
        && renderer.requested_surface_height == surface_height
        && internal_render_width == surface_width
        && internal_render_height == surface_height
        && presented_size == Some((surface_width, surface_height));

    let mut flags =
        SURFACE_PRESENTATION_DYNAMIC_RESOLUTION_DISABLED | SURFACE_PRESENTATION_UPSCALING_DISABLED;
    flags |= u32::from(renderer.last_frame_presented) * SURFACE_PRESENTATION_LAST_FRAME_PRESENTED;
    flags |= u32::from(renderer.ever_presented) * SURFACE_PRESENTATION_EVER_PRESENTED;
    flags |= u32::from(full_resolution) * SURFACE_PRESENTATION_FULL_RESOLUTION;

    GsplatSurfacePresentation {
        requested_width: renderer.requested_surface_width,
        requested_height: renderer.requested_surface_height,
        surface_width,
        surface_height,
        internal_render_width,
        internal_render_height,
        presented_width,
        presented_height,
        presented_camera_revision: renderer.last_presented_camera_revision,
        flags,
        reserved: 0,
    }
}

fn normalize_quaternion_f32(quaternion: [f32; 4]) -> [f32; 4] {
    let norm2 = quaternion[0] * quaternion[0]
        + quaternion[1] * quaternion[1]
        + quaternion[2] * quaternion[2]
        + quaternion[3] * quaternion[3];
    if norm2 <= 0.0 || !norm2.is_finite() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inverse_norm = 1.0 / norm2.sqrt();
    quaternion.map(|value| value * inverse_norm)
}

fn canonical_view_matrix_f32(camera: Camera) -> [f32; 16] {
    // This is the same normalized inverse-quaternion rotation used by the
    // Surface render parameters. Keep the arithmetic explicitly f32: the
    // receipt describes what the runtime adopted, not the source trace's f64
    // oracle values.
    let [x, y, z, w] = normalize_quaternion_f32(camera.pose.rotation_xyzw);
    let [x, y, z, w] = normalize_quaternion_f32([-x, -y, -z, w]);
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    let rotation = [
        1.0 - 2.0 * (yy + zz),
        2.0 * (xy - wz),
        2.0 * (xz + wy),
        2.0 * (xy + wz),
        1.0 - 2.0 * (xx + zz),
        2.0 * (yz - wx),
        2.0 * (xz - wy),
        2.0 * (yz + wx),
        1.0 - 2.0 * (xx + yy),
    ];
    let position = [
        camera.pose.position.x,
        camera.pose.position.y,
        camera.pose.position.z,
    ];
    let translation = std::array::from_fn::<_, 3, _>(|row| {
        -rotation[row * 3 + 2].mul_add(
            position[2],
            rotation[row * 3 + 1].mul_add(position[1], rotation[row * 3] * position[0]),
        )
    });
    [
        rotation[0],
        rotation[1],
        rotation[2],
        translation[0],
        rotation[3],
        rotation[4],
        rotation[5],
        translation[1],
        rotation[6],
        rotation[7],
        rotation[8],
        translation[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn canonical_projection_matrix_f32(camera: Camera, aspect: f32) -> [f32; 16] {
    let focal = 1.0 / (camera.intrinsics.vertical_fov_radians * 0.5).tan();
    let projection_aspect = camera.intrinsics.effective_projection_aspect(aspect);
    let depth =
        camera.intrinsics.far_plane / (camera.intrinsics.far_plane - camera.intrinsics.near_plane);
    [
        focal / projection_aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        focal,
        0.0,
        0.0,
        0.0,
        0.0,
        depth,
        -camera.intrinsics.near_plane * depth,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

fn multiply_mat4_f32(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        left[row * 4 + 3].mul_add(
            right[12 + column],
            left[row * 4 + 2].mul_add(
                right[8 + column],
                left[row * 4 + 1].mul_add(right[4 + column], left[row * 4] * right[column]),
            ),
        )
    })
}

fn surface_camera_receipt(renderer: &GsplatSurfaceRenderer) -> GsplatSurfaceCameraReceiptV1 {
    let camera = renderer.session.camera();
    let camera_revision = renderer.session.camera_revision();
    let (surface_width, surface_height) = renderer.session.surface_size();
    let aspect = surface_width as f32 / surface_height.max(1) as f32;
    let view_matrix = canonical_view_matrix_f32(camera);
    let projection_matrix = canonical_projection_matrix_f32(camera, aspect);
    let current_revision_presented = renderer.last_frame_presented
        && renderer.last_presented_camera_revision == camera_revision
        && renderer.last_sort_stats.camera_revision == camera_revision;
    let mut flags =
        u32::from(renderer.last_frame_presented) * SURFACE_CAMERA_RECEIPT_FRAME_PRESENTED;
    flags |=
        u32::from(current_revision_presented) * SURFACE_CAMERA_RECEIPT_CURRENT_REVISION_PRESENTED;

    GsplatSurfaceCameraReceiptV1 {
        camera_revision,
        presented_camera_revision: renderer.last_presented_camera_revision,
        surface_width,
        surface_height,
        flags,
        position: [
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
        ],
        rotation_xyzw: camera.pose.rotation_xyzw,
        vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
        near_plane: camera.intrinsics.near_plane,
        far_plane: camera.intrinsics.far_plane,
        view_matrix,
        projection_matrix,
        view_projection_matrix: multiply_mat4_f32(projection_matrix, view_matrix),
        ..Default::default()
    }
}

fn create_surface_renderer_from_raw_handles(
    renderer: Renderer,
    raw_display_handle: wgpu::rwh::RawDisplayHandle,
    raw_window_handle: wgpu::rwh::RawWindowHandle,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    let camera_control = match auto_surface_camera_control(&renderer) {
        Ok(camera_control) => camera_control,
        Err(code) => {
            return ffi_error(
                code,
                "gsplat_surface_renderer_create: failed to create automatic camera",
            );
        }
    };
    let camera = surface_camera_from_control(camera_control, renderer.config());
    let mut session = match unsafe {
        SurfaceRenderSession::from_raw_handles(
            renderer,
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            camera,
        )
    } {
        Ok(session) => session,
        Err(err) => {
            log_surface_error(&format!(
                "SurfaceRenderSession::from_raw_handles failed: {err:?}"
            ));
            return ffi_error_display(err.code(), "SurfaceRenderSession::from_raw_handles", err);
        }
    };

    let exactness = match surface_exactness_receipt(&session) {
        Ok(exactness) => exactness,
        Err(message) => return ffi_error(ErrorCode::Internal, message),
    };
    if session.geometry_path() != GeometryPath::PagedActiveAtlas
        && let Err(err) = session.set_order_backend(SurfaceOrderBackend::Adaptive)
    {
        return ffi_error_display(err.code(), "gsplat_surface_renderer_set_order_backend", err);
    }
    let surface_renderer = Box::new(GsplatSurfaceRenderer {
        session,
        exactness,
        requested_surface_width: width,
        requested_surface_height: height,
        last_presented_camera_revision: 0,
        last_frame_presented: false,
        ever_presented: false,
        camera_control,
        render_error_logged: false,
        last_sort_stats: GsplatSurfaceSortStats::default(),
        #[cfg(any(target_os = "android", target_os = "ios"))]
        benchmark_camera_trace: None,
    });
    unsafe {
        *out_renderer = Box::into_raw(surface_renderer);
    }

    ffi_ok()
}

/// Destroy a Surface renderer.
///
/// # Safety
///
/// `renderer` may be null. Non-null values must be handles returned by a
/// Surface renderer create function that have not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_destroy(renderer: *mut GsplatSurfaceRenderer) {
    ffi_catch_void("gsplat_surface_renderer_destroy", || {
        if renderer.is_null() {
            return;
        }

        unsafe {
            drop(Box::from_raw(renderer));
        }
    });
}

/// Resize a Surface renderer.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_resize(
    renderer: *mut GsplatSurfaceRenderer,
    width: u32,
    height: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_resize", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_resize: renderer is null",
                );
            }
        };

        if width == 0 || height == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_resize: width and height must be positive",
            );
        }

        if let Err(err) = renderer.session.resize(width, height) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_resize", err);
        }
        renderer.camera_control = match auto_surface_camera_control(renderer.session.renderer()) {
            Ok(camera_control) => camera_control,
            Err(code) => {
                return ffi_error(
                    code,
                    "gsplat_surface_renderer_resize: failed to reset automatic camera",
                );
            }
        };
        let camera = surface_camera_from_control(
            renderer.camera_control,
            renderer.session.renderer().config(),
        );
        if let Err(err) = renderer.session.set_camera(camera) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_resize", err);
        }
        renderer.requested_surface_width = width;
        renderer.requested_surface_height = height;
        renderer.last_frame_presented = false;
        renderer.render_error_logged = false;

        ffi_ok()
    })
}

/// Set the Surface renderer sort interval in frames.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_sort_interval(
    renderer: *mut GsplatSurfaceRenderer,
    interval: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_sort_interval", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_sort_interval: renderer is null",
                );
            }
        };

        if interval == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_sort_interval: interval must be positive",
            );
        }

        if let Err(err) = renderer.session.set_sort_interval(interval) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_set_sort_interval", err);
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Set the Surface renderer geometry path.
///
/// `path` must be `GSPLAT_GEOMETRY_PATH_DIRECT` (0),
/// `GSPLAT_GEOMETRY_PATH_PACKED_ATLAS` (1), or
/// `GSPLAT_GEOMETRY_PATH_PAGED_ACTIVE_ATLAS` (2). This is an experimental A/B
/// benchmark knob. Mobile constructors default to the exact packed path;
/// Direct remains the wide-float oracle and Paged is diagnostic-only.
/// Construction may select any of the three paths. At runtime, a same-path
/// call is idempotent; any transition entering or leaving Packed returns
/// [`ErrorCode::Unsupported`] before resource preparation or state mutation.
/// Native Direct/Paged changes retain their existing transactional rule, so a
/// failed target preparation leaves the published path live. Packed upload
/// handoff is not reversible.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_geometry_path(
    renderer: *mut GsplatSurfaceRenderer,
    path: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_geometry_path", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_geometry_path: renderer is null",
                );
            }
        };

        let geometry_path = match geometry_path_from_ffi(path) {
            Some(path) => path,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_geometry_path: unsupported path",
                );
            }
        };

        if let Err(err) = renderer.session.set_geometry_path(geometry_path) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_set_geometry_path", err);
        }
        update_surface_exactness_for_geometry_path(&mut renderer.exactness, geometry_path);
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

fn geometry_path_from_ffi(path: u32) -> Option<GeometryPath> {
    match path {
        0 => Some(GeometryPath::SortedIndexDirect),
        1 => Some(GeometryPath::PackedAtlas),
        2 => Some(GeometryPath::PagedActiveAtlas),
        _ => None,
    }
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// This legacy preprojection knob no longer selects ordering behavior. Use
/// `gsplat_surface_renderer_set_order_backend` for CPU/GPU/adaptive ordering
/// and `gsplat_surface_renderer_set_geometry_path` for geometry selection.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_preproject(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_gpu_preproject", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_gpu_preproject: renderer is null",
                );
            }
        };

        let _ = enabled;
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_preproject_double_buffer(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_set_gpu_preproject_double_buffer",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        "gsplat_surface_renderer_set_gpu_preproject_double_buffer: renderer is null",
                    );
                }
            };

            let _ = enabled;
            renderer.render_error_logged = false;
            ffi_ok()
        },
    )
}

/// Enable or disable experimental async sorting.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_async_sort(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_async_sort", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_async_sort: renderer is null",
                );
            }
        };

        if let Err(err) = renderer.session.set_async_sort_enabled(enabled != 0) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_set_async_sort", err);
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Select whether Surface order refreshes run on the CPU, GPU, or adaptive
/// runtime policy.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function. `backend` must be a `GsplatSurfaceOrderBackend` value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_order_backend(
    renderer: *mut GsplatSurfaceRenderer,
    backend: u32,
) -> i32 {
    unsafe {
        set_surface_order_backend(
            renderer,
            backend,
            "gsplat_surface_renderer_set_order_backend",
        )
    }
}

/// Select Candidate, Compact, or Adaptive projected-draw execution.
///
/// Policy zero preserves the pre-v1/default behavior and maps to Adaptive;
/// values 1, 2, and 3 select Candidate, Compact, and Adaptive respectively.
/// A rejected Compact request is transactional and leaves the live policy and
/// learned lanes untouched.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_projected_policy_v1(
    renderer: *mut GsplatSurfaceRenderer,
    policy: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_projected_policy_v1", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_projected_policy_v1: renderer is null",
                );
            }
        };
        let Some(policy) = surface_projected_policy_from_ffi(policy) else {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_projected_policy_v1: unsupported policy",
            );
        };
        if let Err(err) = renderer.session.set_projected_draw_policy(policy) {
            return ffi_error_display(
                err.code(),
                "gsplat_surface_renderer_set_projected_policy_v1",
                err,
            );
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Select the Packed GPU order producer used by an explicit A/B diagnostic.
///
/// Zero and one select the unchanged qualified PostSort default; two selects
/// Preproject. Producer measurement must be disabled while changing graphs so
/// no ticket can cross a generation boundary.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_order_producer_v1(
    renderer: *mut GsplatSurfaceRenderer,
    producer: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_gpu_order_producer_v1", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_gpu_order_producer_v1: renderer is null",
                );
            }
        };
        let Some(producer) = surface_gpu_producer_from_ffi(producer) else {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_gpu_order_producer_v1: unsupported producer",
            );
        };
        // Idempotent reapplication does not cross a graph generation and is
        // safe while strict diagnostics are active. Only a real producer
        // transition must first stop issuing new measurement tickets.
        if renderer.session.gpu_order_producer() == producer {
            return ffi_ok();
        }
        if renderer.session.gpu_producer_measurement_enabled() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                concat!(
                    "gsplat_surface_renderer_set_gpu_order_producer_v1: disable producer ",
                    "measurement before changing graphs"
                ),
            );
        }
        if let Err(error) = renderer.session.set_gpu_order_producer(producer) {
            return ffi_error_display(
                error.code(),
                "gsplat_surface_renderer_set_gpu_order_producer_v1",
                error,
            );
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Enable or disable strict ticketed producer diagnostics.
///
/// The renderer admits `enabled=1` only in the exact Packed +
/// ProjectedQuadsExact + forced Compact context. Ordering remains an
/// independent policy; strict Android qualification additionally requires the
/// forced GPU backend before enabling this lane.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        concat!(
                            "gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1: ",
                            "renderer is null"
                        ),
                    );
                }
            };
            if enabled > 1 {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    concat!(
                        "gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1: ",
                        "enabled must be zero or one"
                    ),
                );
            }
            let enabled = enabled != 0;
            if renderer.session.gpu_producer_measurement_enabled() == enabled {
                return ffi_ok();
            }
            if let Err(error) = renderer
                .session
                .set_gpu_producer_measurement_enabled(enabled)
            {
                return ffi_error_display(
                    error.code(),
                    "gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1",
                    error,
                );
            }
            renderer.render_error_logged = false;
            ffi_ok()
        },
    )
}

/// Compatibility alias retained for existing Android benchmark APKs.
///
/// # Safety
///
/// `renderer` must be a live Surface renderer for the duration of this call.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_android_benchmark_set_order_backend(
    renderer: *mut GsplatSurfaceRenderer,
    backend: u32,
) -> i32 {
    unsafe {
        set_surface_order_backend(
            renderer,
            backend,
            "gsplat_android_benchmark_set_order_backend",
        )
    }
}

/// Compatibility alias retained for existing mobile qualification apps.
///
/// # Safety
///
/// `renderer` must be a live Surface renderer for the duration of this call.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_benchmark_set_surface_order_backend(
    renderer: *mut GsplatSurfaceRenderer,
    backend: u32,
) -> i32 {
    unsafe {
        set_surface_order_backend(
            renderer,
            backend,
            "gsplat_benchmark_set_surface_order_backend",
        )
    }
}

unsafe fn set_surface_order_backend(
    renderer: *mut GsplatSurfaceRenderer,
    backend: u32,
    operation: &'static str,
) -> i32 {
    ffi_catch_i32(operation, || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: renderer is null"),
                );
            }
        };
        let backend = match backend {
            0 => SurfaceOrderBackend::Cpu,
            1 => SurfaceOrderBackend::Gpu,
            2 => SurfaceOrderBackend::Adaptive,
            _ => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: unsupported backend"),
                );
            }
        };
        if let Err(err) = renderer.session.set_order_backend(backend) {
            return ffi_error_display(err.code(), operation, err);
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Mobile example-only fixed camera-trace entrypoint used by qualification
/// benchmarks. It is intentionally absent from the published v0.1 header.
///
/// # Safety
///
/// `renderer` must be a live Surface renderer and `trace_path` must point to a
/// readable nul-terminated UTF-8 string for the duration of this call.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_benchmark_set_surface_camera_trace_frame(
    renderer: *mut GsplatSurfaceRenderer,
    trace_path: *const c_char,
    frame_index: u32,
) -> i32 {
    unsafe {
        set_surface_camera_trace_frame_with_display_policy(
            renderer,
            trace_path,
            frame_index,
            1,
            "gsplat_benchmark_set_surface_camera_trace_frame",
        )
    }
}

/// Mobile example-only trace entrypoint with an explicit display policy.
///
/// `require_trace_display_match == 1` is the formal qualification policy.
/// `0` permits native-aspect reprojection for smoke testing only. This symbol
/// remains absent from the published v0.1 header.
///
/// # Safety
///
/// `renderer` must be a live Surface renderer and `trace_path` must point to a
/// readable nul-terminated UTF-8 string for the duration of this call.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy(
    renderer: *mut GsplatSurfaceRenderer,
    trace_path: *const c_char,
    frame_index: u32,
    require_trace_display_match: u32,
) -> i32 {
    unsafe {
        set_surface_camera_trace_frame_with_display_policy(
            renderer,
            trace_path,
            frame_index,
            require_trace_display_match,
            "gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy",
        )
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
unsafe fn set_surface_camera_trace_frame_with_display_policy(
    renderer: *mut GsplatSurfaceRenderer,
    trace_path: *const c_char,
    frame_index: u32,
    require_trace_display_match: u32,
    operation: &'static str,
) -> i32 {
    ffi_catch_i32(operation, || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: renderer is null"),
                );
            }
        };
        let require_trace_display_match = match require_trace_display_match {
            0 => false,
            1 => true,
            _ => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: require_trace_display_match must equal 0 or 1"),
                );
            }
        };
        if trace_path.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                format!("{operation}: trace_path is null"),
            );
        }
        let path = match unsafe { CStr::from_ptr(trace_path) }.to_str() {
            Ok(path) if !path.is_empty() => path,
            _ => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: invalid trace path"),
                );
            }
        };
        let needs_load = renderer
            .benchmark_camera_trace
            .as_ref()
            .is_none_or(|cached| cached.path != path);
        if needs_load {
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let code = if error.kind() == std::io::ErrorKind::NotFound {
                        ErrorCode::NotFound
                    } else {
                        ErrorCode::Internal
                    };
                    return ffi_error(code, format!("{operation}: cannot read trace: {error}"));
                }
            };
            let trace = match CameraTrace::from_json_slice(&bytes) {
                Ok(trace) => trace,
                Err(error) => {
                    return ffi_error(
                        ErrorCode::ParseFailed,
                        format!("{operation}: invalid trace: {error}"),
                    );
                }
            };
            renderer.benchmark_camera_trace = Some(BenchmarkCameraTraceCache {
                path: path.to_owned(),
                trace,
            });
        }
        let trace = &renderer
            .benchmark_camera_trace
            .as_ref()
            .expect("camera trace cache was initialized")
            .trace;
        if require_trace_display_match {
            let (surface_width, surface_height) = renderer.session.surface_size();
            if (trace.display.width, trace.display.height) != (surface_width, surface_height) {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        concat!(
                            "{}: trace display {}x{} must match Surface {}x{}; ",
                            "use a matching trace for formal qualification"
                        ),
                        operation,
                        trace.display.width,
                        trace.display.height,
                        surface_width,
                        surface_height,
                    ),
                );
            }
        }
        let frame = match trace.frame(frame_index as usize) {
            Ok(frame) => frame,
            Err(error) => {
                return ffi_error_display(ErrorCode::InvalidArgument, operation, error);
            }
        };
        let camera = match frame.camera() {
            Ok(camera) => camera,
            Err(error) => {
                return ffi_error_display(ErrorCode::InvalidArgument, operation, error);
            }
        };
        if let Err(error) = renderer.session.set_camera(camera) {
            return ffi_error_display(error.code(), operation, error);
        }
        renderer.session.force_sort_refresh();
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_async_geometry(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_async_geometry", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_async_geometry: renderer is null",
                );
            }
        };

        let _ = enabled;
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_instance_buffer_count(
    renderer: *mut GsplatSurfaceRenderer,
    count: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_instance_buffer_count", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_instance_buffer_count: renderer is null",
                );
            }
        };

        if count == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_instance_buffer_count: count must be positive",
            );
        }

        let _ = count;
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Set the preferred Surface frame latency.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_frame_latency(
    renderer: *mut GsplatSurfaceRenderer,
    latency: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_frame_latency", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_frame_latency: renderer is null",
                );
            }
        };

        if latency == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_frame_latency: latency must be positive",
            );
        }

        renderer.session.set_frame_latency(latency);
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Reset the Surface camera to the automatic scene framing.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_reset_camera(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_reset_camera", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_reset_camera: renderer is null",
                );
            }
        };

        renderer.camera_control = match auto_surface_camera_control(renderer.session.renderer()) {
            Ok(camera_control) => camera_control,
            Err(code) => {
                return ffi_error(
                    code,
                    "gsplat_surface_renderer_reset_camera: failed to create automatic camera",
                );
            }
        };
        renderer.session.force_sort_refresh();
        apply_surface_camera_control(renderer)
    })
}

/// Orbit the Surface camera by yaw and pitch deltas in radians.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_orbit(
    renderer: *mut GsplatSurfaceRenderer,
    delta_yaw_radians: f32,
    delta_pitch_radians: f32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_orbit", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_orbit: renderer is null",
                );
            }
        };

        if !delta_yaw_radians.is_finite() || !delta_pitch_radians.is_finite() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_orbit: deltas must be finite",
            );
        }

        renderer.camera_control.yaw += delta_yaw_radians;
        renderer.camera_control.pitch = (renderer.camera_control.pitch + delta_pitch_radians)
            .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
        apply_surface_camera_control(renderer)
    })
}

/// Zoom the Surface camera by a positive distance scale.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_zoom(
    renderer: *mut GsplatSurfaceRenderer,
    distance_scale: f32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_zoom", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_zoom: renderer is null",
                );
            }
        };

        if !distance_scale.is_finite() || distance_scale <= 0.0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_zoom: distance_scale must be finite and positive",
            );
        }

        let min_distance =
            (renderer.camera_control.radius * SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER).max(0.01);
        let max_distance = (renderer.camera_control.radius
            * SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER)
            .max(min_distance);
        renderer.camera_control.distance =
            (renderer.camera_control.distance * distance_scale).clamp(min_distance, max_distance);
        apply_surface_camera_control(renderer)
    })
}

/// Pan the Surface camera in normalized viewport units.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_pan(
    renderer: *mut GsplatSurfaceRenderer,
    normalized_delta_x: f32,
    normalized_delta_y: f32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_pan", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_pan: renderer is null",
                );
            }
        };

        if !normalized_delta_x.is_finite() || !normalized_delta_y.is_finite() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_pan: deltas must be finite",
            );
        }

        let config = renderer.session.renderer().config();
        let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
        let view_height = 2.0
            * renderer.camera_control.distance
            * (renderer.session.camera().intrinsics.vertical_fov_radians * 0.5).tan();
        let view_width = view_height * aspect;
        let camera = surface_camera_from_control(renderer.camera_control, config);
        let (right, up, _) = camera_basis(&camera, renderer.camera_control.target);

        renderer.camera_control.target = vec3_add(
            renderer.camera_control.target,
            vec3_add(
                vec3_scale(right, -normalized_delta_x * view_width),
                vec3_scale(up, normalized_delta_y * view_height),
            ),
        );
        apply_surface_camera_control(renderer)
    })
}

/// Render one frame to the Surface renderer.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_render_frame(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_render_frame", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_render_frame: renderer is null",
                );
            }
        };

        match renderer.session.render_frame() {
            Ok(output) => {
                renderer.last_frame_presented = output.frame_presented;
                if output.frame_presented {
                    renderer.last_presented_camera_revision = output.camera_revision;
                    renderer.ever_presented = true;
                }
                renderer.last_sort_stats = output.into();
                renderer.render_error_logged = false;
                ffi_ok()
            }
            Err(err) => {
                renderer.last_frame_presented = false;
                if !renderer.render_error_logged {
                    log_surface_error(&format!(
                        "gsplat_surface_renderer_render_frame failed: {err:?}"
                    ));
                    renderer.render_error_logged = true;
                }
                ffi_error_display(err.code(), "gsplat_surface_renderer_render_frame", err)
            }
        }
    })
}

/// Boundedly advances callbacks for Surface queue work submitted before this
/// call. This does not acquire a Surface, render, submit queue work, or issue a
/// benchmark receipt ticket. A timeout is reported as success because the
/// caller owns the finite retry bound and determines terminal completeness by
/// polling the existing receipt lanes.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_pump_receipts(
    renderer: *mut GsplatSurfaceRenderer,
    timeout_ns: u64,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_pump_receipts", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_pump_receipts: renderer is null",
                );
            }
        };
        match renderer
            .session
            .pump_receipts(Duration::from_nanos(timeout_ns))
        {
            Ok(_) => ffi_ok(),
            Err(error) => {
                ffi_error_display(error.code(), "gsplat_surface_renderer_pump_receipts", error)
            }
        }
    })
}

const SURFACE_RECEIPT_PUMP_QUEUE_COMPLETE: u32 = 1;
const SURFACE_RECEIPT_PUMP_TIMEOUT: u32 = 2;

/// Boundedly advances callbacks and reports whether the native wait observed
/// queue completion or elapsed. Unlike the legacy entrypoint, v1 makes a
/// timeout observable without treating it as an FFI error.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_status` must be
/// null or point to writable `u32` storage. It is not mutated on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_pump_receipts_v1(
    renderer: *mut GsplatSurfaceRenderer,
    timeout_ns: u64,
    out_status: *mut u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_pump_receipts_v1", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_pump_receipts_v1: renderer is null",
                );
            }
        };
        if out_status.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_pump_receipts_v1: out_status is null",
            );
        }
        match renderer
            .session
            .pump_receipts(Duration::from_nanos(timeout_ns))
        {
            Ok(completed) => {
                unsafe {
                    *out_status = if completed {
                        SURFACE_RECEIPT_PUMP_QUEUE_COMPLETE
                    } else {
                        SURFACE_RECEIPT_PUMP_TIMEOUT
                    };
                }
                ffi_ok()
            }
            Err(error) => ffi_error_display(
                error.code(),
                "gsplat_surface_renderer_pump_receipts_v1",
                error,
            ),
        }
    })
}

/// Request one current-stats sample from the next eligible Exact Surface
/// frame. Sampling admission is returned in `out_request->status`; a legal
/// call returns `GSPLAT_OK` even when the Renderer reports an unsampled status.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_request` must
/// describe a writable, initialized v1 structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_request_current_stats_v1(
    renderer: *mut GsplatSurfaceRenderer,
    out_request: *mut GsplatSurfaceCurrentStatsRequestV1,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_request_current_stats_v1", || {
        if let Err(code) = validate_versioned_output(
            out_request,
            SURFACE_CURRENT_STATS_ABI_VERSION_V1,
            "gsplat_surface_renderer_request_current_stats_v1",
        ) {
            return code;
        }
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_request_current_stats_v1: renderer is null",
                );
            }
        };
        let request =
            surface_current_stats_request_to_ffi(renderer.session.request_current_stats());
        unsafe {
            *out_request = request;
        }
        ffi_ok()
    })
}

/// Copy the current frame's presentation-committed current-stats submission.
/// Only `ISSUED` makes the ticket and identity payload applicable.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_submission` must
/// describe a writable, initialized v1 structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_current_stats_submission_v1(
    renderer: *const GsplatSurfaceRenderer,
    out_submission: *mut GsplatSurfaceCurrentStatsSubmissionV1,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_get_current_stats_submission_v1",
        || {
            if let Err(code) = validate_versioned_output(
                out_submission,
                SURFACE_CURRENT_STATS_ABI_VERSION_V1,
                "gsplat_surface_renderer_get_current_stats_submission_v1",
            ) {
                return code;
            }
            let renderer = match unsafe { renderer.as_ref() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        concat!(
                            "gsplat_surface_renderer_get_current_stats_submission_v1: ",
                            "renderer is null"
                        ),
                    );
                }
            };
            let submission = surface_current_stats_submission_to_ffi(
                renderer.session.current_stats_submission(),
            );
            unsafe {
                *out_submission = submission;
            }
            ffi_ok()
        },
    )
}

/// Poll and consume at most one global current-stats resolution or atomic
/// terminal. `READY` contains inseparable ticket, identity, S/V/C/D and count
/// semantics. Terminal failure kinds retain ticket/identity but no counts.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_poll` must
/// describe a writable, initialized v1 structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_current_stats_v1(
    renderer: *mut GsplatSurfaceRenderer,
    out_poll: *mut GsplatSurfaceCurrentStatsPollV1,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_poll_current_stats_v1", || {
        // Validate before polling because a successful poll consumes at most
        // one Renderer-owned resolution or terminal.
        if let Err(code) = validate_versioned_output(
            out_poll,
            SURFACE_CURRENT_STATS_ABI_VERSION_V1,
            "gsplat_surface_renderer_poll_current_stats_v1",
        ) {
            return code;
        }
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_current_stats_v1: renderer is null",
                );
            }
        };
        let poll = surface_current_stats_poll_to_ffi(renderer.session.poll_current_stats());
        unsafe {
            *out_poll = poll;
        }
        ffi_ok()
    })
}

/// Poll and consume at most one global current-stats resolution or atomic
/// terminal. V2 extends the same single-pop receipt with same-ticket timing;
/// it does not create a second ticket, queue, or terminal.
///
/// `READY` always makes frame-complete timing valid. CPU phase values are
/// applicable only when their corresponding validity flags are set. Other
/// poll kinds carry zero validity flags and zero timing payload.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_poll` must
/// describe a writable, initialized v2 structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_current_stats_v2(
    renderer: *mut GsplatSurfaceRenderer,
    out_poll: *mut GsplatSurfaceCurrentStatsPollV2,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_poll_current_stats_v2", || {
        // Validate before polling because a successful poll consumes at most
        // one Renderer-owned resolution or terminal.
        if let Err(code) = validate_versioned_output(
            out_poll,
            SURFACE_CURRENT_STATS_ABI_VERSION_V2,
            "gsplat_surface_renderer_poll_current_stats_v2",
        ) {
            return code;
        }
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_current_stats_v2: renderer is null",
                );
            }
        };
        let poll = surface_current_stats_poll_to_ffi_v2(renderer.session.poll_current_stats());
        unsafe {
            *out_poll = poll;
        }
        ffi_ok()
    })
}

/// Copy the last Surface renderer stats with demonstrably current counts.
///
/// Asynchronous counts require a ticket- and generation-matched current-stats
/// v1 `READY` receipt. This legacy getter returns [`ErrorCode::NotFound`] for
/// unrequested, pending, failed, expired or mismatched counts without modifying
/// `out_stats`. Synchronous current counts retain the historical success path.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function. `out_stats` must be valid for one `GsplatStats` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_stats(
    renderer: *const GsplatSurfaceRenderer,
    out_stats: *mut GsplatStats,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_stats", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_stats: renderer is null",
                );
            }
        };

        if out_stats.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_stats: out_stats is null",
            );
        }

        let out_stats = unsafe { &mut *out_stats };
        if let Err(code) = copy_legacy_surface_stats(renderer.session.legacy_stats(), out_stats) {
            return ffi_error(
                code,
                concat!(
                    "gsplat_surface_renderer_get_stats: current Surface counts ",
                    "are not available from a matching receipt",
                ),
            );
        }

        ffi_ok()
    })
}

/// Copy the exactness and adapter-admission receipt for the renderer's current
/// geometry path. A successful geometry-path switch updates this receipt.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_exactness` must be
/// valid for one `GsplatSurfaceExactness` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_exactness(
    renderer: *const GsplatSurfaceRenderer,
    out_exactness: *mut GsplatSurfaceExactness,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_exactness", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_exactness: renderer is null",
                );
            }
        };
        if out_exactness.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_exactness: out_exactness is null",
            );
        }
        unsafe {
            *out_exactness = renderer.exactness;
        }
        ffi_ok()
    })
}

/// Copy the native pixel-resolution and presentation receipt.
///
/// The internal render size is the renderer's real target size. This native
/// path does not use dynamic resolution or an upscaler. `FULL_RESOLUTION` is
/// set only when the last frame was actually presented and requested, Surface,
/// internal-render, and presented dimensions all match.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_presentation` must
/// be valid for one `GsplatSurfacePresentation` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_presentation(
    renderer: *const GsplatSurfaceRenderer,
    out_presentation: *mut GsplatSurfacePresentation,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_presentation", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_presentation: renderer is null",
                );
            }
        };
        if out_presentation.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_presentation: out_presentation is null",
            );
        }
        unsafe {
            *out_presentation = surface_presentation_receipt(renderer);
        }
        ffi_ok()
    })
}

/// Copy the f32 camera state and canonical matrices used by the current
/// Surface session.
///
/// The caller initializes the v1 header. The receipt remains available for
/// diagnostics before presentation, but strict benchmark evidence requires
/// both camera-receipt flags and equal current/presented revisions.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_receipt` must
/// describe a writable, initialized v1 structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_camera_receipt_v1(
    renderer: *const GsplatSurfaceRenderer,
    out_receipt: *mut GsplatSurfaceCameraReceiptV1,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_camera_receipt_v1", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_camera_receipt_v1: renderer is null",
                );
            }
        };
        if let Err(code) =
            validate_v1_output(out_receipt, "gsplat_surface_renderer_get_camera_receipt_v1")
        {
            return code;
        }
        unsafe {
            *out_receipt = surface_camera_receipt(renderer);
        }
        ffi_ok()
    })
}

/// Copy bounded async-sort telemetry for the last Surface frame.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_stats` must be
/// valid for one `GsplatSurfaceSortStats` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_sort_stats(
    renderer: *const GsplatSurfaceRenderer,
    out_stats: *mut GsplatSurfaceSortStats,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_sort_stats", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_sort_stats: renderer is null",
                );
            }
        };
        if out_stats.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_sort_stats: out_stats is null",
            );
        }
        unsafe {
            *out_stats = renderer.last_sort_stats;
        }
        ffi_ok()
    })
}

/// Copy the CPU/GPU measurement submission identity for the last successful render call.
///
/// A measured refresh with `TICKET_ISSUED` has a non-zero ticket that must
/// terminate in exactly one success/failure receipt. Either UNSAMPLED reason
/// means no ticket was issued and a strict benchmark must reject that refresh.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_submission` must be
/// valid for one `GsplatSurfaceOrderSubmission` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_order_submission(
    renderer: *const GsplatSurfaceRenderer,
    out_submission: *mut GsplatSurfaceOrderSubmission,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_order_submission", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_order_submission: renderer is null",
                );
            }
        };
        if out_submission.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_order_submission: out_submission is null",
            );
        }
        let submission = match renderer
            .session
            .compatibility_submission(SurfaceCompatibilityChannel::Order)
        {
            Some(SurfaceCompatibilitySubmission::Order(submission)) => {
                surface_order_submission_to_ffi(submission)
            }
            None => GsplatSurfaceOrderSubmission::default(),
            Some(_) => unreachable!("order compatibility channel returned another submission"),
        };
        unsafe {
            *out_submission = submission;
        }
        ffi_ok()
    })
}

/// Copy the projected-draw submission identity for the last successful render call.
///
/// The caller must initialize `out_submission->struct_size` to at least
/// `sizeof(GsplatSurfaceProjectedSubmissionV1)` and `version` to 1.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_submission` must
/// describe a writable, initialized v1 structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_projected_submission_v1(
    renderer: *const GsplatSurfaceRenderer,
    out_submission: *mut GsplatSurfaceProjectedSubmissionV1,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_get_projected_submission_v1",
        || {
            let renderer = match unsafe { renderer.as_ref() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        "gsplat_surface_renderer_get_projected_submission_v1: renderer is null",
                    );
                }
            };
            if let Err(code) = validate_v1_output(
                out_submission,
                "gsplat_surface_renderer_get_projected_submission_v1",
            ) {
                return code;
            }
            let submission = match renderer
                .session
                .compatibility_submission(SurfaceCompatibilityChannel::Projected)
            {
                Some(SurfaceCompatibilitySubmission::Projected(submission)) => {
                    surface_projected_submission_to_ffi(submission)
                }
                None => GsplatSurfaceProjectedSubmissionV1::default(),
                Some(_) => {
                    unreachable!("projected compatibility channel returned another submission")
                }
            };
            unsafe { *out_submission = submission };
            ffi_ok()
        },
    )
}

/// Drain one terminal projected-draw success without blocking.
///
/// The caller initializes the v1 header. When the queue is empty this writes
/// an otherwise-zero v1 receipt and sets `out_available` to zero.
///
/// # Safety
///
/// All pointers must be null or valid for the documented writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_projected_measurement_v1(
    renderer: *mut GsplatSurfaceRenderer,
    out_measurement: *mut GsplatSurfaceProjectedMeasurementV1,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_poll_projected_measurement_v1",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        "gsplat_surface_renderer_poll_projected_measurement_v1: renderer is null",
                    );
                }
            };
            if out_available.is_null() {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_projected_measurement_v1: out_available is null",
                );
            }
            if let Err(code) = validate_v1_output(
                out_measurement,
                "gsplat_surface_renderer_poll_projected_measurement_v1",
            ) {
                return code;
            }
            let measurement = match renderer
                .session
                .poll_compatibility_terminal(SurfaceCompatibilityTerminalSelector::ProjectedSuccess)
            {
                SurfaceCompatibilityTerminalPoll::Ready(
                    SurfaceCompatibilityTerminal::ProjectedSuccess(measurement),
                ) => Some(surface_projected_measurement_to_ffi(measurement)),
                SurfaceCompatibilityTerminalPoll::Unavailable(_) => None,
                SurfaceCompatibilityTerminalPoll::Ready(_) => {
                    unreachable!("projected-success selector returned another terminal")
                }
            };
            unsafe {
                *out_measurement = measurement.unwrap_or_default();
                *out_available = u32::from(measurement.is_some());
            }
            ffi_ok()
        },
    )
}

/// Take the V/C/D receipt for one successful projected-draw ticket.
///
/// # Safety
///
/// All pointers must be null or valid for the documented writes. `ticket`
/// must be non-zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_take_projected_counts_v1(
    renderer: *mut GsplatSurfaceRenderer,
    ticket: u64,
    out_counts: *mut GsplatSurfaceProjectedCountsV1,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_take_projected_counts_v1", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_take_projected_counts_v1: renderer is null",
                );
            }
        };
        if ticket == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_take_projected_counts_v1: ticket is zero",
            );
        }
        if out_available.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_take_projected_counts_v1: out_available is null",
            );
        }
        if let Err(code) = validate_v1_output(
            out_counts,
            "gsplat_surface_renderer_take_projected_counts_v1",
        ) {
            return code;
        }
        let ticket = NonZeroU64::new(ticket).expect("non-zero ticket validated above");
        let counts = ready_compatibility_counts(
            renderer
                .session
                .take_compatibility_counts(SurfaceCompatibilityCountFamily::Projected, ticket),
        )
        .map(surface_projected_counts);
        unsafe {
            *out_counts = counts.unwrap_or_default();
            *out_available = u32::from(counts.is_some());
        }
        ffi_ok()
    })
}

/// Drain one terminal projected-draw failure without blocking.
///
/// # Safety
///
/// All pointers must be null or valid for the documented writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_projected_failure_v1(
    renderer: *mut GsplatSurfaceRenderer,
    out_failure: *mut GsplatSurfaceProjectedFailureV1,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_poll_projected_failure_v1", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_projected_failure_v1: renderer is null",
                );
            }
        };
        if out_available.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_poll_projected_failure_v1: out_available is null",
            );
        }
        if let Err(code) = validate_v1_output(
            out_failure,
            "gsplat_surface_renderer_poll_projected_failure_v1",
        ) {
            return code;
        }
        let failure = match renderer
            .session
            .poll_compatibility_terminal(SurfaceCompatibilityTerminalSelector::ProjectedFailure)
        {
            SurfaceCompatibilityTerminalPoll::Ready(
                SurfaceCompatibilityTerminal::ProjectedFailure(failure),
            ) => Some(surface_projected_failure_to_ffi(failure)),
            SurfaceCompatibilityTerminalPoll::Unavailable(_) => None,
            SurfaceCompatibilityTerminalPoll::Ready(_) => {
                unreachable!("projected-failure selector returned another terminal")
            }
        };
        unsafe {
            *out_failure = failure.unwrap_or_default();
            *out_available = u32::from(failure.is_some());
        }
        ffi_ok()
    })
}

/// Copy producer identity for the last successful Surface render call.
///
/// # Safety
///
/// `renderer` must be null or live. `out_submission` must contain an
/// initialized v1 header and be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_gpu_producer_submission_v1(
    renderer: *const GsplatSurfaceRenderer,
    out_submission: *mut GsplatSurfaceGpuProducerSubmissionV1,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_get_gpu_producer_submission_v1",
        || {
            let renderer = match unsafe { renderer.as_ref() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        concat!(
                            "gsplat_surface_renderer_get_gpu_producer_submission_v1: ",
                            "renderer is null"
                        ),
                    );
                }
            };
            if let Err(code) = validate_v1_output(
                out_submission,
                "gsplat_surface_renderer_get_gpu_producer_submission_v1",
            ) {
                return code;
            }
            let submission = match renderer
                .session
                .compatibility_submission(SurfaceCompatibilityChannel::Producer)
            {
                Some(SurfaceCompatibilitySubmission::Producer(submission)) => {
                    surface_gpu_producer_submission_to_ffi(submission)
                }
                None => GsplatSurfaceGpuProducerSubmissionV1::default(),
                Some(_) => {
                    unreachable!("producer compatibility channel returned another submission")
                }
            };
            unsafe { *out_submission = submission };
            ffi_ok()
        },
    )
}

/// Drain one terminal producer success with exact S/C/D counts.
///
/// # Safety
///
/// All pointers must be null or valid for the documented writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_gpu_producer_measurement_v1(
    renderer: *mut GsplatSurfaceRenderer,
    out_measurement: *mut GsplatSurfaceGpuProducerMeasurementV1,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_poll_gpu_producer_measurement_v1",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        concat!(
                            "gsplat_surface_renderer_poll_gpu_producer_measurement_v1: ",
                            "renderer is null"
                        ),
                    );
                }
            };
            if out_available.is_null() {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    concat!(
                        "gsplat_surface_renderer_poll_gpu_producer_measurement_v1: ",
                        "out_available is null"
                    ),
                );
            }
            if let Err(code) = validate_v1_output(
                out_measurement,
                "gsplat_surface_renderer_poll_gpu_producer_measurement_v1",
            ) {
                return code;
            }
            // Raw producer delivery is lossless and independent of the bounded
            // compatibility view. Return an already queued oldest record
            // before asking the session to advance completion callbacks.
            let measurement = renderer
                .session
                .pop_gpu_producer_measurement()
                .or_else(|| {
                    renderer.session.poll_order_measurement_receipts();
                    renderer.session.pop_gpu_producer_measurement()
                })
                .map(surface_gpu_producer_measurement_to_ffi);
            unsafe {
                *out_measurement = measurement.unwrap_or_default();
                *out_available = u32::from(measurement.is_some());
            }
            ffi_ok()
        },
    )
}

/// Drain one terminal producer failure.
///
/// # Safety
///
/// All pointers must be null or valid for the documented writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_gpu_producer_failure_v1(
    renderer: *mut GsplatSurfaceRenderer,
    out_failure: *mut GsplatSurfaceGpuProducerFailureV1,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_poll_gpu_producer_failure_v1",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        concat!(
                            "gsplat_surface_renderer_poll_gpu_producer_failure_v1: ",
                            "renderer is null"
                        ),
                    );
                }
            };
            if out_available.is_null() {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    concat!(
                        "gsplat_surface_renderer_poll_gpu_producer_failure_v1: ",
                        "out_available is null"
                    ),
                );
            }
            if let Err(code) = validate_v1_output(
                out_failure,
                "gsplat_surface_renderer_poll_gpu_producer_failure_v1",
            ) {
                return code;
            }
            let failure = renderer
                .session
                .pop_gpu_producer_measurement_failure()
                .or_else(|| {
                    renderer.session.poll_order_measurement_receipts();
                    renderer.session.pop_gpu_producer_measurement_failure()
                })
                .map(surface_gpu_producer_failure_to_ffi);
            unsafe {
                *out_failure = failure.unwrap_or_default();
                *out_available = u32::from(failure.is_some());
            }
            ffi_ok()
        },
    )
}

/// Drain the oldest completed CPU order measurement without blocking.
///
/// The receipt reports frame-start through graphics-queue completion, not CPU
/// submit-wall time. It is therefore directly comparable with GPU completion
/// evidence. A successful call writes `1` to `out_available` when a receipt was
/// written, or `0` when the queue is empty.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_measurement` and
/// `out_available` must each be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_cpu_order_measurement(
    renderer: *mut GsplatSurfaceRenderer,
    out_measurement: *mut GsplatSurfaceCpuOrderMeasurement,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_poll_cpu_order_measurement", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_cpu_order_measurement: renderer is null",
                );
            }
        };
        if out_measurement.is_null() || out_available.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_poll_cpu_order_measurement: output is null",
            );
        }

        let measurement = match renderer
            .session
            .poll_compatibility_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess)
        {
            SurfaceCompatibilityTerminalPoll::Ready(
                SurfaceCompatibilityTerminal::OrderCpuSuccess(measurement),
            ) => Some(surface_cpu_order_measurement_to_ffi(measurement)),
            SurfaceCompatibilityTerminalPoll::Unavailable(_) => None,
            SurfaceCompatibilityTerminalPoll::Ready(_) => {
                unreachable!("CPU-order selector returned another terminal")
            }
        };
        unsafe {
            *out_measurement = measurement.unwrap_or_default();
            *out_available = u32::from(measurement.is_some());
        }
        ffi_ok()
    })
}

/// Drain the oldest completed GPU order measurement without blocking.
///
/// A successful call writes `1` to `out_available` and one receipt to
/// `out_measurement`, or writes `0` and a zeroed receipt when no GPU work has
/// completed yet. Repeated calls drain all currently available receipts in
/// ticket order.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_measurement` and
/// `out_available` must each be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_order_measurement(
    renderer: *mut GsplatSurfaceRenderer,
    out_measurement: *mut GsplatSurfaceOrderMeasurement,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_poll_order_measurement", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_order_measurement: renderer is null",
                );
            }
        };
        if out_measurement.is_null() || out_available.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_poll_order_measurement: output is null",
            );
        }

        let measurement = match renderer
            .session
            .poll_compatibility_terminal(SurfaceCompatibilityTerminalSelector::OrderGpuSuccess)
        {
            SurfaceCompatibilityTerminalPoll::Ready(
                SurfaceCompatibilityTerminal::OrderGpuSuccess(measurement),
            ) => Some(surface_order_measurement_to_ffi(measurement)),
            SurfaceCompatibilityTerminalPoll::Unavailable(_) => None,
            SurfaceCompatibilityTerminalPoll::Ready(_) => {
                unreachable!("GPU-order selector returned another terminal")
            }
        };
        unsafe {
            *out_measurement = measurement.unwrap_or_default();
            *out_available = u32::from(measurement.is_some());
        }
        ffi_ok()
    })
}

/// Take the terminal V/C/D receipt for one successful CPU or GPU ticket.
///
/// The ledger is bounded and each receipt is returned at most once. Consumers
/// must join on both `ticket` and `camera_revision`; this avoids associating a
/// moving-camera frame with counts completed for an older revision.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_counts` and
/// `out_available` must each be valid for one write. `ticket` must be non-zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_take_order_counts(
    renderer: *mut GsplatSurfaceRenderer,
    ticket: u64,
    out_counts: *mut GsplatSurfaceOrderCounts,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_take_order_counts", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_take_order_counts: renderer is null",
                );
            }
        };
        if ticket == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_take_order_counts: ticket is zero",
            );
        }
        if out_counts.is_null() || out_available.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_take_order_counts: output is null",
            );
        }

        let ticket = NonZeroU64::new(ticket).expect("non-zero ticket validated above");
        let counts = ready_compatibility_counts(
            renderer
                .session
                .take_compatibility_counts(SurfaceCompatibilityCountFamily::Order, ticket),
        )
        .map(surface_order_counts);
        unsafe {
            *out_counts = counts.unwrap_or_default();
            *out_available = u32::from(counts.is_some());
        }
        ffi_ok()
    })
}

/// Drain the oldest terminal CPU/GPU order-measurement failure without blocking.
///
/// A successful call writes `1` to `out_available` and one failure receipt to
/// `out_failure`, or writes `0` and a zeroed receipt when none is available.
/// Every issued CPU/GPU measurement ticket eventually appears in exactly one of
/// the success or failure polling queues while the renderer remains live and
/// continues to be pumped.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_failure` and
/// `out_available` must each be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_poll_order_measurement_failure(
    renderer: *mut GsplatSurfaceRenderer,
    out_failure: *mut GsplatSurfaceOrderMeasurementFailure,
    out_available: *mut u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_poll_order_measurement_failure",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        "gsplat_surface_renderer_poll_order_measurement_failure: renderer is null",
                    );
                }
            };
            if out_failure.is_null() || out_available.is_null() {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_poll_order_measurement_failure: output is null",
                );
            }

            let failure = match renderer
                .session
                .poll_compatibility_terminal(SurfaceCompatibilityTerminalSelector::OrderFailure)
            {
                SurfaceCompatibilityTerminalPoll::Ready(
                    SurfaceCompatibilityTerminal::OrderFailure(failure),
                ) => Some(surface_order_measurement_failure_to_ffi(failure)),
                SurfaceCompatibilityTerminalPoll::Unavailable(_) => None,
                SurfaceCompatibilityTerminalPoll::Ready(_) => {
                    unreachable!("order-failure selector returned another terminal")
                }
            };
            unsafe {
                *out_failure = failure.unwrap_or_default();
                *out_available = u32::from(failure.is_some());
            }
            ffi_ok()
        },
    )
}

fn auto_camera(renderer: &Renderer) -> Result<Camera, ErrorCode> {
    let camera_control = auto_surface_camera_control(renderer)?;
    Ok(surface_camera_from_control(
        camera_control,
        renderer.config(),
    ))
}

fn auto_surface_camera_control(renderer: &Renderer) -> Result<SurfaceCameraControl, ErrorCode> {
    let Some(positions) = renderer.positions() else {
        return Err(ErrorCode::SceneNotLoaded);
    };
    let Some((min, max)) = scene_bounds(positions) else {
        return Err(ErrorCode::InvalidArgument);
    };

    let config = renderer.config();
    let center = Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    );
    let extent = Vec3f::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let half_x = (extent.x * 0.5).max(1e-3);
    let half_y = (extent.y * 0.5).max(1e-3);
    let half_z = (extent.z * 0.5).max(1e-3);

    let aspect = (config.width as f32) / (config.height as f32);
    let vfov = Camera::default().intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    let dist = (dist_y.max(dist_x) + half_z) * 1.2;
    let radius = half_x.max(half_y).max(half_z);

    Ok(SurfaceCameraControl {
        target: center,
        radius,
        yaw: 0.0,
        pitch: 0.0,
        distance: dist,
    })
}

fn apply_surface_camera_control(renderer: &mut GsplatSurfaceRenderer) -> i32 {
    let camera = surface_camera_from_control(
        renderer.camera_control,
        renderer.session.renderer().config(),
    );
    if let Err(err) = renderer.session.set_camera(camera) {
        return ffi_error_display(err.code(), "apply_surface_camera_control", err);
    }

    renderer.render_error_logged = false;
    ffi_ok()
}

fn surface_camera_from_control(control: SurfaceCameraControl, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    let pitch = control
        .pitch
        .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
    let cos_pitch = pitch.cos();
    let offset = Vec3f::new(
        control.yaw.sin() * cos_pitch * control.distance,
        pitch.sin() * control.distance,
        -control.yaw.cos() * cos_pitch * control.distance,
    );
    camera.pose.position = vec3_add(control.target, offset);
    camera.pose.rotation_xyzw = camera_rotation_looking_at(camera.pose.position, control.target);

    let radius = control.radius.max(1.0e-3);
    camera.intrinsics.near_plane = (control.distance - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (control.distance + radius * 8.0).max(100.0);

    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
    if aspect < 0.6 {
        camera.intrinsics.vertical_fov_radians = 65.0_f32.to_radians();
    }

    camera
}

fn camera_rotation_looking_at(position: Vec3f, target: Vec3f) -> [f32; 4] {
    let (_, _, forward) = camera_basis_from_position(position, target);
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    quat_from_camera_basis(right, up, forward)
}

fn camera_basis(camera: &Camera, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    camera_basis_from_position(camera.pose.position, target)
}

fn camera_basis_from_position(position: Vec3f, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let forward = vec3_normalize(vec3_sub(target, position)).unwrap_or(Vec3f::new(0.0, 0.0, 1.0));
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    (right, up, forward)
}

fn quat_from_camera_basis(right: Vec3f, up: Vec3f, forward: Vec3f) -> [f32; 4] {
    let m00 = right.x;
    let m01 = up.x;
    let m02 = forward.x;
    let m10 = right.y;
    let m11 = up.y;
    let m12 = forward.y;
    let m20 = right.z;
    let m21 = up.z;
    let m22 = forward.z;
    let trace = m00 + m11 + m22;

    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        ((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s)
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        (0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s)
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        ((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s)
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        ((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s)
    };

    let norm = (x * x + y * y + z * z + w * w).sqrt().max(1.0e-6);
    [x / norm, y / norm, z / norm, w / norm]
}

fn vec3_add(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn vec3_sub(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn vec3_scale(v: Vec3f, scale: f32) -> Vec3f {
    Vec3f::new(v.x * scale, v.y * scale, v.z * scale)
}

fn vec3_cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn vec3_normalize(v: Vec3f) -> Option<Vec3f> {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if !len.is_finite() || len <= 1.0e-6 {
        return None;
    }

    Some(vec3_scale(v, 1.0 / len))
}

fn scene_bounds(positions: &[Vec3f]) -> Option<(Vec3f, Vec3f)> {
    if positions.is_empty() {
        return None;
    }
    let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }
    Some((min, max))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::path::Path;
    use std::ptr;

    use gsplat_core::{ErrorCode, RendererConfig};
    use gsplat_render_wgpu::{
        GeometryPath, Renderer, SurfaceAdaptiveGpuFailureReason, SurfaceAdaptiveState,
        SurfaceCompatibilityCountFamily, SurfaceCompatibilityCounts,
        SurfaceCompatibilityCountsTake, SurfaceCompatibilityCountsUnavailable,
        SurfaceCompatibilityCountsUnavailableReason, SurfaceCompatibilityOrderCpuSuccess,
        SurfaceCompatibilityOrderFailure, SurfaceCompatibilityOrderGpuSuccess,
        SurfaceCompatibilityOrderIssueContext, SurfaceCompatibilityProjectedIssueContext,
        SurfaceCompatibilityProjectedSuccess, SurfaceOrderBackend, SurfaceOrderBackendUsed,
        SurfaceOrderMeasurementFailureReason, SurfaceProjectedDrawAdaptiveState,
        SurfaceProjectedDrawExecution, SurfaceProjectedDrawPolicy, SurfaceTimingSource,
    };

    use super::{
        GsplatCamera, GsplatConfig, GsplatContext, GsplatSurfaceCameraReceiptV1,
        GsplatSurfaceCpuOrderMeasurement, GsplatSurfaceExactness, GsplatSurfaceOrderCounts,
        GsplatSurfaceOrderMeasurement, GsplatSurfaceOrderMeasurementFailure,
        GsplatSurfaceOrderSubmission, GsplatSurfacePresentation, GsplatSurfaceProjectedCountsV1,
        GsplatSurfaceProjectedFailureV1, GsplatSurfaceProjectedMeasurementV1,
        GsplatSurfaceProjectedSubmissionV1, SURFACE_EXACTNESS_FULL_QUALITY_FLAGS,
        SURFACE_EXACTNESS_SAMPLING_DISABLED, SurfaceCameraControl, camera_rotation_looking_at,
        canonical_projection_matrix_f32, canonical_view_matrix_f32, copy_legacy_surface_stats,
        ffi_catch_i32, geometry_path_from_ffi, gsplat_camera_default, gsplat_config_default,
        gsplat_context_create, gsplat_context_destroy, gsplat_context_get_stats,
        gsplat_context_load_scene_path, gsplat_context_render_frame,
        gsplat_context_set_auto_camera, gsplat_context_set_camera, gsplat_error_message,
        gsplat_last_error_message, gsplat_surface_renderer_get_camera_receipt_v1,
        gsplat_surface_renderer_get_exactness, gsplat_surface_renderer_get_order_submission,
        gsplat_surface_renderer_get_presentation, gsplat_surface_renderer_get_stats,
        gsplat_surface_renderer_orbit, gsplat_surface_renderer_pan,
        gsplat_surface_renderer_poll_cpu_order_measurement,
        gsplat_surface_renderer_poll_order_measurement,
        gsplat_surface_renderer_poll_order_measurement_failure,
        gsplat_surface_renderer_pump_receipts_v1, gsplat_surface_renderer_render_frame,
        gsplat_surface_renderer_reset_camera, gsplat_surface_renderer_resize,
        gsplat_surface_renderer_set_async_geometry, gsplat_surface_renderer_set_async_sort,
        gsplat_surface_renderer_set_frame_latency, gsplat_surface_renderer_set_geometry_path,
        gsplat_surface_renderer_set_gpu_preproject,
        gsplat_surface_renderer_set_gpu_preproject_double_buffer,
        gsplat_surface_renderer_set_instance_buffer_count,
        gsplat_surface_renderer_set_order_backend, gsplat_surface_renderer_set_sort_interval,
        gsplat_surface_renderer_take_order_counts, gsplat_surface_renderer_zoom,
        load_ply_path_into_renderer, multiply_mat4_f32, ready_compatibility_counts,
        surface_adaptive_gpu_failure_flags, surface_camera_from_control,
        surface_cpu_order_measurement_to_ffi, surface_order_counts,
        surface_order_measurement_failure_to_ffi, surface_order_measurement_to_ffi,
        surface_projected_counts, surface_projected_measurement_to_ffi,
        surface_projected_policy_from_ffi, update_surface_exactness_for_geometry_path,
        validate_v1_output,
    };

    #[test]
    fn default_ffi_values_match_release_contract() {
        let config = gsplat_config_default();
        assert_eq!(config.width, 1280);
        assert_eq!(config.height, 720);
        assert_eq!(config.mode, 0);

        let camera = gsplat_camera_default();
        assert_eq!(camera.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(camera.near_plane, 0.01);
        assert_eq!(camera.far_plane, 1000.0);
        assert_eq!(std::mem::size_of::<super::GsplatSurfaceSortStats>(), 48);
        assert_eq!(std::mem::size_of::<GsplatSurfaceOrderMeasurement>(), 64);
        assert_eq!(std::mem::size_of::<GsplatSurfaceCpuOrderMeasurement>(), 48);
        assert_eq!(std::mem::size_of::<GsplatSurfaceOrderCounts>(), 32);
        assert_eq!(
            std::mem::size_of::<GsplatSurfaceOrderMeasurementFailure>(),
            40
        );
        assert_eq!(std::mem::size_of::<GsplatSurfaceOrderSubmission>(), 32);
        assert_eq!(
            std::mem::size_of::<GsplatSurfaceProjectedSubmissionV1>(),
            48
        );
        assert_eq!(
            std::mem::size_of::<GsplatSurfaceProjectedMeasurementV1>(),
            56
        );
        assert_eq!(std::mem::size_of::<GsplatSurfaceProjectedCountsV1>(), 40);
        assert_eq!(std::mem::size_of::<GsplatSurfaceProjectedFailureV1>(), 56);
        assert_eq!(
            std::mem::size_of::<super::GsplatSurfaceGpuProducerSubmissionV1>(),
            48
        );
        assert_eq!(
            std::mem::size_of::<super::GsplatSurfaceGpuProducerMeasurementV1>(),
            72
        );
        assert_eq!(
            std::mem::size_of::<super::GsplatSurfaceGpuProducerFailureV1>(),
            56
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceProjectedSubmissionV1, ticket),
            8
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceProjectedSubmissionV1, flags),
            40
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceProjectedMeasurementV1, frame_complete_ms),
            40
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceProjectedCountsV1, visible_count),
            24
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceProjectedFailureV1, reason),
            40
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceProjectedSubmissionV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceProjectedMeasurementV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceProjectedCountsV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceProjectedFailureV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::offset_of!(super::GsplatSurfaceGpuProducerSubmissionV1, ticket),
            8
        );
        assert_eq!(
            std::mem::offset_of!(
                super::GsplatSurfaceGpuProducerMeasurementV1,
                frame_complete_ms
            ),
            40
        );
        assert_eq!(
            std::mem::offset_of!(super::GsplatSurfaceGpuProducerFailureV1, reason),
            40
        );
        assert_eq!(
            std::mem::align_of::<super::GsplatSurfaceGpuProducerSubmissionV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<super::GsplatSurfaceGpuProducerMeasurementV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<super::GsplatSurfaceGpuProducerFailureV1>(),
            std::mem::align_of::<u64>()
        );
        assert_eq!(std::mem::size_of::<GsplatSurfaceExactness>(), 64);
        assert_eq!(std::mem::size_of::<GsplatSurfacePresentation>(), 48);
        assert_eq!(std::mem::size_of::<GsplatSurfaceCameraReceiptV1>(), 272);
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCameraReceiptV1, camera_revision),
            8
        );
        assert_eq!(
            std::mem::offset_of!(GsplatSurfaceCameraReceiptV1, view_matrix),
            80
        );
        assert_eq!(
            std::mem::align_of::<GsplatSurfaceCameraReceiptV1>(),
            std::mem::align_of::<u64>()
        );
    }

    #[test]
    fn canonical_camera_receipt_matrices_follow_row_major_projection_times_view() {
        let camera = gsplat_core::Camera::default();
        let view = canonical_view_matrix_f32(camera);
        assert_eq!(
            view,
            [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        );
        let projection = canonical_projection_matrix_f32(camera, 16.0 / 9.0);
        let view_projection = multiply_mat4_f32(projection, view);
        assert_eq!(view_projection, projection);
        assert!(projection[0] > 0.0 && projection[5] > projection[0]);
        assert_eq!(projection[14], 1.0);
    }

    #[test]
    fn canonical_projection_receipt_uses_centered_focal_length_ratio() {
        let mut camera = gsplat_core::Camera::default();
        camera.intrinsics.focal_length_x_over_y = 581.924_56 / 578.670_1;
        let viewport_aspect = 979.0 / 546.0;
        let projection = canonical_projection_matrix_f32(camera, viewport_aspect);

        assert!(
            (projection[0] / projection[5]
                - camera.intrinsics.focal_length_x_over_y / viewport_aspect)
                .abs()
                <= 1.0e-6
        );

        let legacy: gsplat_core::Camera = GsplatCamera::default().into();
        assert_eq!(legacy.intrinsics.focal_length_x_over_y, 1.0);
        assert_eq!(
            canonical_projection_matrix_f32(legacy, viewport_aspect)[0],
            canonical_projection_matrix_f32(gsplat_core::Camera::default(), viewport_aspect)[0]
        );
    }

    #[test]
    fn legacy_surface_stats_fail_closed_without_output_mutation() {
        let mut output = super::GsplatStats {
            frame_ms: -1.0,
            preprocess_ms: -2.0,
            sort_ms: -3.0,
            raster_ms: -4.0,
            visible_count: u32::MAX - 1,
            drawn_count: u32::MAX,
        };
        let before = output;

        assert_eq!(
            copy_legacy_surface_stats(None, &mut output),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(output.frame_ms.to_bits(), before.frame_ms.to_bits());
        assert_eq!(
            output.preprocess_ms.to_bits(),
            before.preprocess_ms.to_bits()
        );
        assert_eq!(output.sort_ms.to_bits(), before.sort_ms.to_bits());
        assert_eq!(output.raster_ms.to_bits(), before.raster_ms.to_bits());
        assert_eq!(output.visible_count, before.visible_count);
        assert_eq!(output.drawn_count, before.drawn_count);
    }

    #[test]
    fn legacy_surface_stats_preserve_current_success_semantics() {
        let stats = gsplat_core::FrameStats {
            frame_ms: 1.0,
            preprocess_ms: 2.0,
            sort_ms: 3.0,
            raster_ms: 4.0,
            visible_count: 5,
            drawn_count: 6,
        };

        let mut output = super::GsplatStats::from(gsplat_core::FrameStats::zero());
        assert_eq!(copy_legacy_surface_stats(Some(stats), &mut output), Ok(()));
        assert_eq!(output.frame_ms, stats.frame_ms);
        assert_eq!(output.preprocess_ms, stats.preprocess_ms);
        assert_eq!(output.sort_ms, stats.sort_ms);
        assert_eq!(output.raster_ms, stats.raster_ms);
        assert_eq!(output.visible_count, stats.visible_count);
        assert_eq!(output.drawn_count, stats.drawn_count);
    }

    #[test]
    fn projected_v1_policy_and_success_receipts_preserve_identity_and_counts() {
        assert_eq!(
            surface_projected_policy_from_ffi(0),
            Some(gsplat_render_wgpu::SurfaceProjectedDrawPolicy::Adaptive)
        );
        assert_eq!(
            surface_projected_policy_from_ffi(1),
            Some(gsplat_render_wgpu::SurfaceProjectedDrawPolicy::Candidate)
        );
        assert_eq!(
            surface_projected_policy_from_ffi(2),
            Some(gsplat_render_wgpu::SurfaceProjectedDrawPolicy::Compact)
        );
        assert_eq!(surface_projected_policy_from_ffi(4), None);

        let measurement = SurfaceCompatibilityProjectedSuccess {
            issue: SurfaceCompatibilityProjectedIssueContext {
                requested_policy: SurfaceProjectedDrawPolicy::Adaptive,
                actual_execution: SurfaceProjectedDrawExecution::Compact,
                order_backend: SurfaceOrderBackendUsed::Gpu,
                adaptive_state: SurfaceProjectedDrawAdaptiveState::CompactProbe,
            },
            ticket: 1_u64 << 52,
            camera_revision: 17,
            execution: SurfaceProjectedDrawExecution::Compact,
            order_backend: SurfaceOrderBackendUsed::Gpu,
            projection_generation: 9,
            probe_generation: 4,
            projection_rebuilt: true,
            order_refreshed: false,
            frame_complete_ms: 3.5,
            exact_contributor_compaction: true,
            dropped_prior: true,
        };
        let receipt = surface_projected_measurement_to_ffi(measurement);
        assert_eq!(
            receipt.struct_size as usize,
            std::mem::size_of_val(&receipt)
        );
        assert_eq!(receipt.version, 1);
        assert_eq!(receipt.ticket, 1_u64 << 52);
        assert_eq!(receipt.execution, 2);
        assert_eq!(receipt.order_backend, 1);
        assert_eq!(receipt.flags, 0b1101);

        let counts = surface_projected_counts(SurfaceCompatibilityCounts {
            family: SurfaceCompatibilityCountFamily::Projected,
            ticket: measurement.ticket,
            camera_revision: measurement.camera_revision,
            visible_count: 123,
            contributor_count: 87,
            drawn_count: 87,
            exact_contributor_compaction: true,
        });
        assert_eq!(
            (
                counts.visible_count,
                counts.contributor_count,
                counts.drawn_count
            ),
            (123, 87, 87)
        );
        assert_eq!(counts.flags, 1);
    }

    #[test]
    fn projected_v1_output_requires_initialized_compatible_header() {
        let mut receipt = GsplatSurfaceProjectedMeasurementV1::default();
        assert!(validate_v1_output(&mut receipt, "test").is_ok());

        receipt.version = 2;
        let invalid_version = receipt;
        assert_eq!(
            validate_v1_output(&mut receipt, "test"),
            Err(ErrorCode::InvalidArgument.as_i32())
        );
        assert_eq!(receipt, invalid_version);
        receipt.version = 1;
        receipt.struct_size -= 1;
        let undersized = receipt;
        assert_eq!(
            validate_v1_output(&mut receipt, "test"),
            Err(ErrorCode::InvalidArgument.as_i32())
        );
        assert_eq!(receipt, undersized);
    }

    #[test]
    fn renderer_owned_submission_values_translate_without_ffi_caches() {
        let order = super::surface_order_submission_to_ffi(
            gsplat_render_wgpu::SurfaceCompatibilityOrderSubmission {
                camera_revision: 77,
                requested_backend: SurfaceOrderBackend::Adaptive,
                actual_backend: SurfaceOrderBackendUsed::Gpu,
                adaptive_state: SurfaceAdaptiveState::GpuProbe,
                measurement: gsplat_render_wgpu::SurfaceOrderMeasurementSubmission::Issued {
                    backend: SurfaceOrderBackendUsed::Gpu,
                    ticket: 19,
                },
            },
        );
        assert_eq!(order.ticket, 19);
        assert_eq!(order.camera_revision, 77);
        assert_eq!(order.requested_backend, 2);
        assert_eq!(order.actual_backend, 1);
        assert_eq!(order.adaptive_state, 3);
        assert_eq!(order.flags, 0b11);

        let projected = super::surface_projected_submission_to_ffi(
            gsplat_render_wgpu::SurfaceCompatibilityProjectedSubmission {
                camera_revision: 78,
                requested_policy: SurfaceProjectedDrawPolicy::Compact,
                actual_execution: SurfaceProjectedDrawExecution::Compact,
                order_backend: SurfaceOrderBackendUsed::Gpu,
                adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
                measurement:
                    gsplat_render_wgpu::SurfaceProjectedDrawMeasurementSubmission::NotRequested,
            },
        );
        assert_eq!(projected.ticket, 0);
        assert_eq!(projected.camera_revision, 78);
        assert_eq!(projected.requested_policy, 2);
        assert_eq!(projected.actual_execution, 2);
        assert_eq!(projected.flags, 0);

        let producer = super::surface_gpu_producer_submission_to_ffi(
            gsplat_render_wgpu::SurfaceCompatibilityProducerSubmission {
                camera_revision: 79,
                requested_producer: gsplat_render_wgpu::SurfaceGpuOrderProducer::Preproject,
                actual_producer: Some(gsplat_render_wgpu::SurfaceGpuOrderProducer::Preproject),
                order_backend: SurfaceOrderBackendUsed::Gpu,
                projected_execution: SurfaceProjectedDrawExecution::Compact,
                measurement_enabled: true,
                measurement:
                    gsplat_render_wgpu::SurfaceGpuProducerMeasurementSubmission::NotRequested,
            },
        );
        assert_eq!(producer.ticket, 0);
        assert_eq!(producer.camera_revision, 79);
        assert_eq!(producer.requested_producer, 2);
        assert_eq!(producer.actual_producer, 2);
        assert_eq!(producer.flags, 0b1000);
    }

    #[test]
    fn compatibility_count_statuses_are_not_reconstructed_by_the_c_layer() {
        let ticket = NonZeroU64::new(9).unwrap();
        let ready = SurfaceCompatibilityCounts {
            family: SurfaceCompatibilityCountFamily::Projected,
            ticket: ticket.get(),
            camera_revision: 17,
            visible_count: 200,
            contributor_count: 150,
            drawn_count: 150,
            exact_contributor_compaction: true,
        };
        assert_eq!(
            ready_compatibility_counts(SurfaceCompatibilityCountsTake::Ready(ready)),
            Some(ready)
        );
        for reason in [
            SurfaceCompatibilityCountsUnavailableReason::Pending,
            SurfaceCompatibilityCountsUnavailableReason::Failed,
            SurfaceCompatibilityCountsUnavailableReason::Expired,
            SurfaceCompatibilityCountsUnavailableReason::Consumed,
            SurfaceCompatibilityCountsUnavailableReason::InvalidTicket,
        ] {
            assert_eq!(
                ready_compatibility_counts(SurfaceCompatibilityCountsTake::Unavailable(
                    SurfaceCompatibilityCountsUnavailable {
                        family: SurfaceCompatibilityCountFamily::Projected,
                        ticket,
                        reason,
                    }
                )),
                None
            );
        }
    }

    #[test]
    fn geometry_path_ids_cover_the_experimental_constructor_contract() {
        assert_eq!(
            geometry_path_from_ffi(0),
            Some(GeometryPath::SortedIndexDirect)
        );
        assert_eq!(geometry_path_from_ffi(1), Some(GeometryPath::PackedAtlas));
        assert_eq!(
            geometry_path_from_ffi(2),
            Some(GeometryPath::PagedActiveAtlas)
        );
        assert_eq!(geometry_path_from_ffi(3), None);
    }

    #[test]
    fn geometry_path_switch_updates_exactness_without_publishing_partial_as_full() {
        let mut exactness = GsplatSurfaceExactness {
            source_splat_count: 42,
            decoded_splat_count: 42,
            encoded_splat_count: 42,
            resident_splat_count: 42,
            addressable_splat_count: 42,
            source_sh_degree: 3,
            resident_sh_degree: 3,
            quality_flags: SURFACE_EXACTNESS_FULL_QUALITY_FLAGS,
            max_storage_buffers_per_shader_stage: 8,
            max_storage_buffer_binding_size: 128 * 1024 * 1024,
        };

        update_surface_exactness_for_geometry_path(&mut exactness, GeometryPath::PagedActiveAtlas);
        assert_eq!(exactness.decoded_splat_count, 42);
        assert_eq!(exactness.encoded_splat_count, 42);
        assert_eq!(exactness.resident_splat_count, 0);
        assert_eq!(exactness.addressable_splat_count, 0);
        assert_eq!(exactness.resident_sh_degree, 0);
        assert_eq!(exactness.quality_flags, SURFACE_EXACTNESS_SAMPLING_DISABLED);

        update_surface_exactness_for_geometry_path(&mut exactness, GeometryPath::PackedAtlas);
        assert_eq!(exactness.resident_splat_count, 42);
        assert_eq!(exactness.addressable_splat_count, 42);
        assert_eq!(exactness.resident_sh_degree, 3);
        assert_eq!(
            exactness.quality_flags,
            SURFACE_EXACTNESS_FULL_QUALITY_FLAGS
        );
        assert_eq!(exactness.max_storage_buffers_per_shader_stage, 8);
        assert_eq!(exactness.max_storage_buffer_binding_size, 128 * 1024 * 1024);
    }

    #[test]
    fn gpu_order_measurement_preserves_timing_validity_and_policy_identity() {
        let receipt = surface_order_measurement_to_ffi(SurfaceCompatibilityOrderGpuSuccess {
            issue: SurfaceCompatibilityOrderIssueContext {
                requested_backend: SurfaceOrderBackend::Adaptive,
                actual_backend: SurfaceOrderBackendUsed::Gpu,
                adaptive_state: SurfaceAdaptiveState::GpuProbe,
            },
            ticket: 41,
            camera_revision: 17,
            timing_source: SurfaceTimingSource::TimestampQuery,
            gpu_preprocess_ms: Some(0.25),
            gpu_radix_ms: Some(1.5),
            gpu_order_ms: Some(1.75),
            gpu_complete_ms: 2.25,
            timestamp_period_ns: Some(1.0),
            below_timestamp_resolution: true,
            visible_count: 123,
            drawn_count: 100,
            exact_contributor_compaction: true,
            dropped_prior: false,
        });

        assert_eq!(receipt.ticket, 41);
        assert_eq!(receipt.camera_revision, 17);
        assert_eq!(receipt.timing_source, 1);
        assert_eq!(receipt.requested_backend, 2);
        assert_eq!(receipt.actual_backend, 1);
        assert_eq!(receipt.adaptive_state, 3);
        assert_eq!(receipt.visible_count, 123);
        assert_eq!(receipt.drawn_count, 100);
        assert_eq!(receipt.flags, 0b101_1111);
        assert_eq!(receipt.gpu_order_ms, 1.75);
    }

    #[test]
    fn completion_only_order_measurement_leaves_timestamp_fields_invalid() {
        let receipt = surface_order_measurement_to_ffi(SurfaceCompatibilityOrderGpuSuccess {
            issue: SurfaceCompatibilityOrderIssueContext {
                requested_backend: SurfaceOrderBackend::Gpu,
                actual_backend: SurfaceOrderBackendUsed::Gpu,
                adaptive_state: SurfaceAdaptiveState::Disabled,
            },
            ticket: 7,
            camera_revision: 3,
            timing_source: SurfaceTimingSource::CompletionOnly,
            gpu_preprocess_ms: None,
            gpu_radix_ms: None,
            gpu_order_ms: None,
            gpu_complete_ms: 4.5,
            timestamp_period_ns: None,
            below_timestamp_resolution: false,
            visible_count: 90,
            drawn_count: 90,
            exact_contributor_compaction: false,
            dropped_prior: false,
        });

        assert_eq!(receipt.timing_source, 2);
        assert_eq!(receipt.flags, 0);
        assert_eq!(receipt.gpu_preprocess_ms, 0.0);
        assert_eq!(receipt.gpu_radix_ms, 0.0);
        assert_eq!(receipt.gpu_order_ms, 0.0);
        assert_eq!(receipt.timestamp_period_ns, 0.0);
        assert_eq!(receipt.gpu_complete_ms, 4.5);
    }

    #[test]
    fn cpu_order_measurement_preserves_queue_completion_and_policy_identity() {
        let receipt = surface_cpu_order_measurement_to_ffi(SurfaceCompatibilityOrderCpuSuccess {
            issue: SurfaceCompatibilityOrderIssueContext {
                requested_backend: SurfaceOrderBackend::Adaptive,
                actual_backend: SurfaceOrderBackendUsed::Cpu,
                adaptive_state: SurfaceAdaptiveState::CpuLearning,
            },
            ticket: 12,
            camera_revision: 5,
            preprocess_ms: 0.75,
            sort_ms: 2.5,
            frame_complete_ms: 5.25,
            contributor_count: 75,
            exact_contributor_compaction: true,
            dropped_prior: false,
        });

        assert_eq!(receipt.ticket, 12);
        assert_eq!(receipt.camera_revision, 5);
        assert_eq!(receipt.preprocess_ms, 0.75);
        assert_eq!(receipt.sort_ms, 2.5);
        assert_eq!(receipt.frame_complete_ms, 5.25);
        assert_eq!(receipt.requested_backend, 2);
        assert_eq!(receipt.actual_backend, 0);
        assert_eq!(receipt.adaptive_state, 1);
        assert_eq!(receipt.flags, 0b110);
        assert_eq!(receipt.reserved, 75);
    }

    #[test]
    fn renderer_owned_order_counts_translate_without_ticket_inference() {
        let counts = surface_order_counts(SurfaceCompatibilityCounts {
            family: SurfaceCompatibilityCountFamily::Order,
            ticket: 2,
            camera_revision: 102,
            visible_count: 200,
            contributor_count: 150,
            drawn_count: 150,
            exact_contributor_compaction: true,
        });
        assert_eq!(counts.camera_revision, 102);
        assert_eq!(
            (
                counts.visible_count,
                counts.contributor_count,
                counts.drawn_count
            ),
            (200, 150, 150),
        );
        assert_eq!(counts.flags, 1);
    }

    #[test]
    fn gpu_order_failure_preserves_terminal_reason_and_policy_identity() {
        let receipt = surface_order_measurement_failure_to_ffi(SurfaceCompatibilityOrderFailure {
            issue: SurfaceCompatibilityOrderIssueContext {
                requested_backend: SurfaceOrderBackend::Adaptive,
                actual_backend: SurfaceOrderBackendUsed::Gpu,
                adaptive_state: SurfaceAdaptiveState::GpuProbe,
            },
            ticket: 9,
            camera_revision: 8,
            reason: SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
            dropped_prior: false,
        });

        assert_eq!(receipt.ticket, 9);
        assert_eq!(receipt.camera_revision, 8);
        assert_eq!(receipt.reason, 2);
        assert_eq!(receipt.requested_backend, 2);
        assert_eq!(receipt.actual_backend, 1);
        assert_eq!(receipt.adaptive_state, 3);
        assert_eq!(receipt.flags, 0);
        assert_eq!(receipt.reserved, 0);
    }

    #[test]
    fn adaptive_gpu_failure_reason_uses_additive_sort_status_bits() {
        assert_eq!(surface_adaptive_gpu_failure_flags(None), 0);
        assert_eq!(
            surface_adaptive_gpu_failure_flags(Some(SurfaceAdaptiveGpuFailureReason::Unsupported)),
            (1 << 15) | (1 << 16)
        );
        assert_eq!(
            surface_adaptive_gpu_failure_flags(Some(SurfaceAdaptiveGpuFailureReason::Validation)),
            (1 << 15) | (4 << 16)
        );
    }

    #[test]
    fn ffi_packed_path_loader_publishes_only_exact_resident_scene() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets/minimal_ascii.ply");
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(GeometryPath::PackedAtlas);

        let summary = load_ply_path_into_renderer(&path, &mut renderer).unwrap();
        assert_eq!(renderer.scene_len(), Some(summary.gaussians));
        assert_eq!(renderer.scene_sh_degree(), Some(summary.sh_degree));
        assert!(renderer.scene().is_none());
        assert!(renderer.resident_scene().is_some());
    }

    #[test]
    fn error_message_describes_known_and_unknown_codes() {
        let invalid = unsafe { std::ffi::CStr::from_ptr(gsplat_error_message(1)) };
        assert_eq!(invalid.to_str().unwrap(), "invalid argument");

        let unknown = unsafe { std::ffi::CStr::from_ptr(gsplat_error_message(-1)) };
        assert_eq!(unknown.to_str().unwrap(), "unknown error");
    }

    #[test]
    fn last_error_message_tracks_recent_failure() {
        let rc = unsafe { gsplat_context_load_scene_path(ptr::null_mut(), ptr::null()) };
        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());

        let detail = unsafe { std::ffi::CStr::from_ptr(gsplat_last_error_message()) };
        assert!(
            detail
                .to_str()
                .unwrap()
                .contains("gsplat_context_load_scene_path")
        );
    }

    #[test]
    fn panic_at_ffi_boundary_returns_internal_error_and_detail() {
        let rc = ffi_catch_i32("gsplat_test_panicking_entrypoint", || {
            panic!("simulated internal panic")
        });

        assert_eq!(rc, ErrorCode::Internal.as_i32());
        let detail = unsafe { std::ffi::CStr::from_ptr(gsplat_last_error_message()) };
        assert_eq!(
            detail.to_str().unwrap(),
            "gsplat_test_panicking_entrypoint: panic caught at C ABI boundary"
        );
    }

    #[test]
    fn surface_camera_control_matches_default_view_direction() {
        let control = SurfaceCameraControl {
            target: gsplat_core::Vec3f::new(1.0, 2.0, 3.0),
            radius: 2.0,
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
        };

        let camera = surface_camera_from_control(control, gsplat_core::RendererConfig::default());

        assert_eq!(
            camera.pose.position,
            gsplat_core::Vec3f::new(1.0, 2.0, -7.0)
        );
        assert_eq!(camera.pose.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn looking_at_rotation_is_normalized_for_orbited_camera() {
        let q = camera_rotation_looking_at(
            gsplat_core::Vec3f::new(2.0, 1.0, -4.0),
            gsplat_core::Vec3f::new(0.0, 0.0, 0.0),
        );
        let norm2 = q.iter().map(|v| v * v).sum::<f32>();

        assert!((norm2 - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn create_and_destroy_context() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();

        let rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn context_create_rejects_null_out_pointer() {
        let rc = unsafe { gsplat_context_create(GsplatConfig::default(), ptr::null_mut()) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
    }

    #[test]
    fn context_create_rejects_non_release_render_mode() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let config = GsplatConfig {
            mode: 1,
            ..GsplatConfig::default()
        };

        let rc = unsafe { gsplat_context_create(config, &mut ctx) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        assert!(ctx.is_null());
    }

    #[test]
    fn context_set_camera_rejects_invalid_intrinsics() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let create_rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(create_rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        let camera = GsplatCamera {
            near_plane: 10.0,
            far_plane: 1.0,
            ..GsplatCamera::default()
        };
        let rc = unsafe { gsplat_context_set_camera(ctx, camera) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn context_functions_reject_null_handles_and_outputs() {
        let mut stats = super::GsplatStats::from(gsplat_core::FrameStats::zero());

        assert_eq!(
            unsafe { gsplat_context_set_auto_camera(ptr::null_mut()) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_load_scene_path(ptr::null_mut(), ptr::null()) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_render_frame(ptr::null_mut()) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_get_stats(ptr::null(), &mut stats) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_get_stats(ptr::null(), ptr::null_mut()) },
            ErrorCode::InvalidArgument.as_i32()
        );
    }

    #[test]
    fn context_load_scene_path_rejects_null_path() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let create_rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(create_rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        let rc = unsafe { gsplat_context_load_scene_path(ctx, ptr::null()) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn context_get_stats_rejects_null_output() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let create_rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(create_rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        let rc = unsafe { gsplat_context_get_stats(ctx, ptr::null_mut()) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn surface_functions_reject_null_renderer() {
        let mut stats = super::GsplatStats::from(gsplat_core::FrameStats::zero());
        let mut measurement = GsplatSurfaceOrderMeasurement::default();
        let mut cpu_measurement = GsplatSurfaceCpuOrderMeasurement::default();
        let mut counts = GsplatSurfaceOrderCounts::default();
        let mut failure = GsplatSurfaceOrderMeasurementFailure::default();
        let mut submission = GsplatSurfaceOrderSubmission::default();
        let mut exactness = GsplatSurfaceExactness::default();
        let mut presentation = GsplatSurfacePresentation::default();
        let mut camera_receipt = GsplatSurfaceCameraReceiptV1::default();
        let mut available = 0_u32;
        let mut pump_status = 77_u32;
        let expected = ErrorCode::InvalidArgument.as_i32();

        assert_eq!(
            unsafe { gsplat_surface_renderer_resize(ptr::null_mut(), 640, 480) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_sort_interval(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_order_backend(ptr::null_mut(), 2) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_gpu_preproject(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_gpu_preproject_double_buffer(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_async_sort(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_async_geometry(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_geometry_path(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_instance_buffer_count(ptr::null_mut(), 2) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_frame_latency(ptr::null_mut(), 2) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_reset_camera(ptr::null_mut()) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_orbit(ptr::null_mut(), 0.1, 0.1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_zoom(ptr::null_mut(), 1.1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_pan(ptr::null_mut(), 0.1, 0.1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_render_frame(ptr::null_mut()) },
            expected
        );
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_pump_receipts_v1(ptr::null_mut(), 1, &mut pump_status)
            },
            expected
        );
        assert_eq!(pump_status, 77, "error must not mutate pump status");
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_stats(ptr::null(), &mut stats) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_stats(ptr::null(), ptr::null_mut()) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_exactness(ptr::null(), &mut exactness) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_presentation(ptr::null(), &mut presentation) },
            expected
        );
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_get_camera_receipt_v1(ptr::null(), &mut camera_receipt)
            },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_order_submission(ptr::null(), &mut submission) },
            expected
        );
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_poll_cpu_order_measurement(
                    ptr::null_mut(),
                    &mut cpu_measurement,
                    &mut available,
                )
            },
            expected
        );
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_poll_order_measurement(
                    ptr::null_mut(),
                    &mut measurement,
                    &mut available,
                )
            },
            expected
        );
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_take_order_counts(
                    ptr::null_mut(),
                    1,
                    &mut counts,
                    &mut available,
                )
            },
            expected
        );
        assert_eq!(
            unsafe {
                gsplat_surface_renderer_poll_order_measurement_failure(
                    ptr::null_mut(),
                    &mut failure,
                    &mut available,
                )
            },
            expected
        );
    }
}
