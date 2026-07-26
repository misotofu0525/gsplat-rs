use gsplat_core::{Camera, FrameStats};
use std::num::NonZeroU64;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use crate::SurfaceFrameCapture;
pub use crate::api::SurfaceOrderBackendUsed;
use crate::evidence::SessionEvidence;
pub use crate::evidence::{
    SurfaceCompatibilityChannel, SurfaceCompatibilityCountFamily, SurfaceCompatibilityCountsTake,
    SurfaceCompatibilitySubmission, SurfaceCompatibilityTerminalPoll,
    SurfaceCompatibilityTerminalSelector, SurfaceGpuProducerMeasurementSubmission,
    SurfaceGpuProducerMeasurementUnsampledReason, SurfaceOrderMeasurementSubmission,
    SurfaceOrderMeasurementUnsampledReason, SurfaceProjectedDrawMeasurementSubmission,
    SurfaceProjectedDrawMeasurementUnsampledReason,
};
use crate::gpu_producer_telemetry::GpuProducerTelemetryPoll;
use crate::gpu_telemetry::{
    CpuOrderTelemetryPoll, GpuOrderTelemetryPoll, SurfaceCpuOrderMeasurement, TelemetrySubmission,
};
use crate::projected_draw_telemetry::ProjectedDrawTelemetryPoll;
#[cfg(not(target_arch = "wasm32"))]
use crate::surface::async_sort_supported;
#[cfg(test)]
use crate::surface::{
    ADAPTIVE_CPU_BOOTSTRAP_SAMPLES, ADAPTIVE_INITIAL_PROBE_DELAY, paged_surface_counts,
    projected_formal_sample_requested, validate_projected_draw_policy_transition,
};
use crate::surface::{
    AdaptiveMetric, AdaptiveOrderPolicy, AdaptiveProbeOwner, AdaptiveProjectedDrawPolicy,
    AdaptiveRefreshChoice, AdaptiveSampleKind, ExactSurfacePlanState,
    LegacySurfaceStatsAvailability, ProjectedAdaptiveChoice, ProjectedAdaptiveSampleKind,
    SessionFrameAttempt, SessionFrameExecutor, SessionSchedule, SessionSurfaceOwner,
    StandaloneCpuFrameAttempt, StandaloneGpuFrameAttempt, SurfaceFramePlan,
    adaptive_primary_metric, arbitrate_new_probe_owner, commit_exact_plan_state,
    commit_projected_draw_policy_transition, defer_projected_formal_choice,
    gpu_producer_measurement_context_is_valid, gpu_projected_order_changed,
    order_probe_owner_should_yield, prepare_exact_gpu_order, prepare_exact_gpu_order_producer,
    projected_order_changed, projected_policy_can_sample, projected_probe_claims_owner,
    reset_adaptive_for_gpu_producer_measurement_transition,
    should_reset_order_for_projected_incumbent_change, validate_gpu_order_producer_transition,
};
pub use crate::surface::{
    SurfaceAdaptiveGpuFailureReason, SurfaceAdaptivePendingSample, SurfaceAdaptiveState,
    SurfaceProjectedDrawAdaptivePendingSample, SurfaceProjectedDrawAdaptiveState,
};
use crate::{
    GeometryPath, Renderer, RendererError, SurfaceCurrentStatsPoll, SurfaceCurrentStatsRequest,
    SurfaceCurrentStatsSubmission, SurfaceGpuOrderProducer, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfacePresenter, SurfacePresenterError, SurfaceProjectedDrawExecution,
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
    SurfaceRasterExecutionPlan, timer_elapsed_ms, timer_now,
};
use crate::{
    plans::PlanId, renderer::ExactAdaptivePolicyState, surface::shadow::SurfaceExactError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSortSchedule {
    Interval(u32),
    AsyncLatest { interval: u32 },
}

/// Selects where a required Direct order refresh is computed. This is
/// independent from [`SurfaceSortSchedule`], which decides *when* to refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceOrderBackend {
    #[default]
    Cpu,
    Gpu,
    Adaptive,
}

/// Selects the exact projected draw execution independently from CPU/GPU
/// ordering. Forced modes are deterministic experiment controls; Adaptive
/// keeps separate learned lanes for orders produced by each backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceProjectedDrawPolicy {
    Candidate,
    Compact,
    #[default]
    Adaptive,
}

impl SurfaceOrderMeasurementSubmission {
    fn from_presenter(backend: SurfaceOrderBackendUsed, submission: TelemetrySubmission) -> Self {
        match submission {
            TelemetrySubmission::NotRequested => Self::NotRequested,
            TelemetrySubmission::Issued(ticket) => Self::Issued { backend, ticket },
            TelemetrySubmission::RingBusy => Self::Unsampled {
                backend,
                reason: SurfaceOrderMeasurementUnsampledReason::RingBusy,
            },
            TelemetrySubmission::SurfaceUnavailable => Self::Unsampled {
                backend,
                reason: SurfaceOrderMeasurementUnsampledReason::SurfaceUnavailable,
            },
            TelemetrySubmission::GpuOrderPreparationPending => Self::NotRequested,
        }
    }
}

impl SurfaceGpuProducerMeasurementSubmission {
    fn from_presenter(
        producer: Option<SurfaceGpuOrderProducer>,
        submission: TelemetrySubmission,
    ) -> Self {
        let Some(producer) = producer else {
            return Self::NotRequested;
        };
        match submission {
            TelemetrySubmission::NotRequested | TelemetrySubmission::GpuOrderPreparationPending => {
                Self::NotRequested
            }
            TelemetrySubmission::Issued(ticket) => Self::Issued { producer, ticket },
            TelemetrySubmission::RingBusy => Self::Unsampled {
                producer,
                reason: SurfaceGpuProducerMeasurementUnsampledReason::RingBusy,
            },
            TelemetrySubmission::SurfaceUnavailable => Self::Unsampled {
                producer,
                reason: SurfaceGpuProducerMeasurementUnsampledReason::SurfaceUnavailable,
            },
        }
    }
}

impl SurfaceProjectedDrawMeasurementSubmission {
    fn from_presenter(
        execution: SurfaceProjectedDrawExecution,
        submission: TelemetrySubmission,
    ) -> Self {
        match submission {
            TelemetrySubmission::NotRequested | TelemetrySubmission::GpuOrderPreparationPending => {
                Self::NotRequested
            }
            TelemetrySubmission::Issued(ticket) => Self::Issued { execution, ticket },
            TelemetrySubmission::RingBusy => Self::Unsampled {
                execution,
                reason: SurfaceProjectedDrawMeasurementUnsampledReason::RingBusy,
            },
            TelemetrySubmission::SurfaceUnavailable => Self::Unsampled {
                execution,
                reason: SurfaceProjectedDrawMeasurementUnsampledReason::SurfaceUnavailable,
            },
        }
    }
}

/// Resets learned timings only when the raster workload actually changes.
/// Bindings commonly re-apply their current configuration; treating that as a
/// transition would discard valid Adaptive evidence and force needless CPU
/// bootstrap samples.
fn reset_adaptive_for_raster_transition(
    policy: &mut AdaptiveOrderPolicy,
    previous: SurfaceRasterExecutionPlan,
    next: SurfaceRasterExecutionPlan,
) -> bool {
    if previous == next {
        return false;
    }
    policy.reset(adaptive_primary_metric());
    true
}

impl SurfaceSortSchedule {
    pub const fn interval(self) -> u32 {
        match self {
            Self::Interval(interval) | Self::AsyncLatest { interval } => interval,
        }
    }
}

fn adaptive_gpu_order_failure_reason(
    error: &RendererError,
) -> Option<SurfaceAdaptiveGpuFailureReason> {
    match error {
        RendererError::SurfacePresenter(crate::SurfacePresenterError::GpuOrderUnsupported)
        | RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::StorageBindingCountUnsupported(_)
            | crate::ResidentGpuError::DispatchLimitExceeded,
        )) => Some(SurfaceAdaptiveGpuFailureReason::Unsupported),
        RendererError::SurfacePresenter(crate::SurfacePresenterError::DirectScene(
            crate::DirectSceneError::GpuOrderInitialization(_),
        ))
        | RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::GpuOrderInitialization(_),
        )) => Some(SurfaceAdaptiveGpuFailureReason::Initialization),
        RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::GpuOrderOutOfMemory(_),
        )) => Some(SurfaceAdaptiveGpuFailureReason::OutOfMemory),
        RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::GpuOrderValidation(_),
        )) => Some(SurfaceAdaptiveGpuFailureReason::Validation),
        _ => None,
    }
}

fn should_measure_cpu_refresh(
    plan: SurfaceFramePlan,
    requested_backend: SurfaceOrderBackendUsed,
    geometry_path: GeometryPath,
) -> bool {
    plan.refresh_sort
        && requested_backend == SurfaceOrderBackendUsed::Cpu
        && geometry_path != GeometryPath::PagedActiveAtlas
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFrameTimings {
    /// Retained for API compatibility. Direct rendering performs no CPU geometry expansion.
    pub cpu_geometry_ms: f32,
    /// CPU wall time spent updating GPU resources, encoding, submitting, and presenting.
    pub render_submit_ms: f32,
    /// End-to-end call wall time for the shared session frame.
    pub frame_wall_ms: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFrameOutput {
    /// CPU ordering phases are populated synchronously for CPU frames. GPU
    /// timing/count evidence is asynchronous and carries its own revision.
    pub stats: FrameStats,
    pub timings: SurfaceFrameTimings,
    /// True only when this call submitted the final raster/blit work and
    /// presented a drawable. GPU order preparation returns false while its
    /// hidden submission or asynchronous count readback is pending.
    pub frame_presented: bool,
    /// Generic retry state for GPU ordering preparation. Callers must retry
    /// the same camera revision after yielding to the event loop and must not
    /// count this call as a rendered frame or a measured order submission.
    pub gpu_order_preparation_pending: bool,
    pub raster_execution_plan: SurfaceRasterExecutionPlan,
    pub sort_refreshed: bool,
    pub order_uploaded: bool,
    /// Camera-revision lag of an async result observed on this frame.
    pub async_sort_revision_lag: Option<u32>,
    /// True when a completed async result exceeded the bounded-lag policy.
    pub stale_async_sort_dropped: bool,
    /// True when a new background sort was launched after this frame.
    pub async_sort_scheduled: bool,
    pub camera_revision: u64,
    pub applied_order_revision: u64,
    pub presented_order_revision_lag: u32,
    pub async_sort_scheduled_revision: Option<u64>,
    pub async_sort_completed_revision: Option<u64>,
    pub async_sort_result_applied: bool,
    pub sync_sort_fallback: bool,
    pub order_backend: SurfaceOrderBackendUsed,
    /// True when a requested GPU refresh failed and the same frame recovered
    /// through the deterministic CPU path.
    pub gpu_sort_fallback: bool,
    pub adaptive_state: SurfaceAdaptiveState,
    pub adaptive_gpu_failure: Option<SurfaceAdaptiveGpuFailureReason>,
    /// Independent exact projected draw strategy used by this frame.
    pub projected_draw_policy: SurfaceProjectedDrawPolicy,
    pub projected_draw_execution: SurfaceProjectedDrawExecution,
    pub projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState,
    pub projected_draw_measurement_submission: SurfaceProjectedDrawMeasurementSubmission,
    pub completed_projected_draw_measurement: Option<SurfaceProjectedDrawMeasurement>,
    pub completed_projected_draw_measurement_failure:
        Option<SurfaceProjectedDrawMeasurementFailure>,
    /// Actual Packed GPU producer used by this presented frame. CPU, Direct,
    /// Paged, and preparation-only calls report `None`.
    pub gpu_order_producer: Option<SurfaceGpuOrderProducer>,
    pub gpu_producer_measurement_submission: SurfaceGpuProducerMeasurementSubmission,
    pub completed_gpu_producer_measurement: Option<SurfaceGpuProducerMeasurement>,
    pub completed_gpu_producer_measurement_failure: Option<SurfaceGpuProducerMeasurementFailure>,
    /// Complete requested/issued/unsampled identity for this frame.
    pub order_measurement_submission: SurfaceOrderMeasurementSubmission,
    /// Measurement scheduled by this frame, if a ring slot was available.
    /// Kept as a compatibility mirror of
    /// [`SurfaceOrderMeasurementSubmission::ticket`].
    pub submitted_measurement_ticket: Option<u64>,
    /// Newest result harvested at the start of this frame. It may describe an
    /// earlier camera revision and must be joined by ticket/revision.
    pub completed_order_measurement: Option<SurfaceOrderMeasurement>,
    /// Newest terminal failure harvested at the start of this frame. Issued
    /// GPU tickets never disappear silently on readback/context invalidation.
    pub completed_order_measurement_failure: Option<SurfaceOrderMeasurementFailure>,
    pub visible_count_revision: Option<u64>,
    pub visible_count_pending: bool,
    pub gpu_timestamp_queries_enabled: bool,
}

// Telemetry polling stays with the Session facade. The S5 frame executor owns
// only one borrowed prepare/encode/submit/present attempt.
impl SessionSurfaceOwner {
    fn gpu_order_timestamps_enabled(&self) -> bool {
        match self {
            Self::Standalone(presenter) => presenter.gpu_order_timestamps_enabled(),
            Self::ExactPacked(host) => host.gpu_order_timestamps_enabled(),
            #[cfg(test)]
            Self::Test(_) => false,
        }
    }

    fn poll_cpu_order_completion_telemetry(&mut self) -> CpuOrderTelemetryPoll {
        match self {
            Self::Standalone(presenter) => presenter.poll_cpu_order_completion_telemetry(),
            Self::ExactPacked(_) => CpuOrderTelemetryPoll {
                completed: Vec::new(),
                failures: Vec::new(),
            },
            #[cfg(test)]
            Self::Test(_) => CpuOrderTelemetryPoll {
                completed: Vec::new(),
                failures: Vec::new(),
            },
        }
    }

    fn poll_gpu_order_telemetry(&mut self) -> GpuOrderTelemetryPoll {
        match self {
            Self::Standalone(presenter) => presenter.poll_gpu_order_telemetry(),
            Self::ExactPacked(_) => GpuOrderTelemetryPoll {
                completed: Vec::new(),
                failures: Vec::new(),
            },
            #[cfg(test)]
            Self::Test(_) => GpuOrderTelemetryPoll {
                completed: Vec::new(),
                failures: Vec::new(),
            },
        }
    }

    fn poll_projected_draw_telemetry(&mut self) -> ProjectedDrawTelemetryPoll {
        ProjectedDrawTelemetryPoll {
            completed: Vec::new(),
            failures: Vec::new(),
        }
    }

    fn poll_gpu_producer_telemetry(&mut self) -> GpuProducerTelemetryPoll {
        GpuProducerTelemetryPoll {
            completed: Vec::new(),
            failures: Vec::new(),
        }
    }
}

/// Owns the ordering + direct GPU draw lifecycle shared by every Surface client.
///
/// PLY-derived scene attributes stay GPU-resident. CPU refreshes upload compact
/// source IDs, while GPU refreshes keep stable `(depth_key, source_id)` pairs on
/// the renderer device and draw their source IDs directly. The vertex shader
/// fetches and projects the corresponding Gaussian for Web, desktop, Android,
/// and iOS.
pub struct SurfaceRenderSession {
    renderer: Renderer,
    presenter: SessionSurfaceOwner,
    exact_plan_receipt: Option<ExactSurfacePlanState>,
    exact_order_generation_receipt: Option<(PlanId, u64)>,
    current_stats_submission: SurfaceCurrentStatsSubmission,
    legacy_stats_availability: LegacySurfaceStatsAvailability,
    camera: Camera,
    schedule: SessionSchedule,
    order_backend: SurfaceOrderBackend,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    /// Configured public policy for Exact. The renderer-owned four-state plan
    /// remains the execution owner, so a forced CPU/GPU plan may execute
    /// Candidate while this receipt correctly remains Adaptive.
    exact_projected_draw_policy_requested: Option<SurfaceProjectedDrawPolicy>,
    gpu_producer_measurement: SurfaceGpuProducerMeasurementControl,
    presented_order_backend: SurfaceOrderBackendUsed,
    gpu_order_initialized: bool,
    adaptive_policy: AdaptiveOrderPolicy,
    adaptive_projected_cpu: AdaptiveProjectedDrawPolicy,
    adaptive_projected_gpu: AdaptiveProjectedDrawPolicy,
    adaptive_probe_owner: Option<AdaptiveProbeOwner>,
    blocked_order_choice: Option<AdaptiveRefreshChoice>,
    adaptive_gpu_failure: Option<SurfaceAdaptiveGpuFailureReason>,
    pending_order_backend: Option<SurfaceOrderBackendUsed>,
    pending_adaptive_choice: Option<AdaptiveRefreshChoice>,
    pending_projected_choice: Option<ProjectedAdaptiveChoice>,
    camera_revision: u64,
    last_stats: FrameStats,
    latest_cpu_order_measurement: Option<SurfaceCpuOrderMeasurement>,
    latest_gpu_order_measurement: Option<SurfaceOrderMeasurement>,
    evidence: SessionEvidence,
    #[cfg(not(target_arch = "wasm32"))]
    pending_async_completion: Option<PendingAsyncCompletion>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct PendingAsyncCompletion {
    accepted_order: Option<PendingAsyncOrder>,
    completed_timing: Option<(f32, f32)>,
    observed_revision_lag: Option<u32>,
    stale_result_dropped: bool,
    completed_revision: Option<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct PendingAsyncOrder {
    camera: Camera,
    camera_revision: u64,
    revision_lag: u32,
}

#[derive(Debug, Default)]
struct SurfaceGpuProducerMeasurementControl {
    enabled: bool,
}

impl SurfaceGpuProducerMeasurementControl {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn transition(
        &mut self,
        enabled: bool,
        context_is_valid: bool,
    ) -> Result<bool, SurfacePresenterError> {
        if self.enabled == enabled {
            return Ok(false);
        }
        if enabled && !context_is_valid {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible);
        }
        self.enabled = enabled;
        Ok(true)
    }
}

fn try_switch_renderer_geometry_path(
    renderer: &mut Renderer,
    target: GeometryPath,
    prepare_presenter: impl FnOnce(&Renderer) -> Result<(), SurfacePresenterError>,
) -> Result<bool, SurfacePresenterError> {
    let previous = renderer.geometry_path();
    if previous == target {
        return Ok(false);
    }
    if previous == GeometryPath::PackedAtlas || target == GeometryPath::PackedAtlas {
        return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported);
    }

    renderer.set_geometry_path(target);
    if let Err(error) = prepare_presenter(renderer) {
        renderer.set_geometry_path(previous);
        return Err(error);
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceGeometrySwitchEntry {
    AlreadyActive,
    Synchronous,
    Unsupported,
}

fn surface_geometry_switch_entry(
    web: bool,
    current: GeometryPath,
    target: GeometryPath,
) -> SurfaceGeometrySwitchEntry {
    if current == target {
        SurfaceGeometrySwitchEntry::AlreadyActive
    } else if web || current == GeometryPath::PackedAtlas || target == GeometryPath::PackedAtlas {
        SurfaceGeometrySwitchEntry::Unsupported
    } else {
        SurfaceGeometrySwitchEntry::Synchronous
    }
}

fn legacy_surface_current_stats_request(_renderer: &mut Renderer) -> SurfaceCurrentStatsRequest {
    SurfaceCurrentStatsRequest::Unsampled(crate::SurfaceCurrentStatsUnsampledReason::GpuUnavailable)
}

fn legacy_surface_current_stats_poll(_renderer: &mut Renderer) -> SurfaceCurrentStatsPoll {
    SurfaceCurrentStatsPoll::Empty
}

#[cfg(test)]
const fn legacy_surface_current_stats_submission() -> SurfaceCurrentStatsSubmission {
    SurfaceCurrentStatsSubmission::NotRequested
}

fn map_surface_exact_error(error: SurfaceExactError) -> RendererError {
    match error {
        SurfaceExactError::Surface(error) => error.into(),
        SurfaceExactError::Frame(error) => crate::map_frame_execution_error(error),
        SurfaceExactError::TargetSizeMismatch {
            requested,
            configured,
            acquired,
        } => SurfacePresenterError::SurfaceConfigure(format!(
            "Exact Surface target mismatch: requested={requested:?}, configured={configured:?}, acquired={acquired:?}"
        ))
        .into(),
    }
}

fn publish_only_on_present<T>(published: &mut T, presented: Option<T>) {
    if let Some(presented) = presented {
        *published = presented;
    }
}

fn exact_order_refreshed(
    published: Option<(PlanId, u64)>,
    plan: PlanId,
    order_generation: u64,
) -> bool {
    // Every complete plan owns an independent monotonic generation domain.
    // A plan transition with the same numeric value still publishes a fresh
    // authoritative order identity.
    !matches!(published, Some((published_plan, published_generation))
        if published_plan == plan && published_generation == order_generation)
}

const fn exact_published_camera_revision(
    session_revision: u64,
    renderer_revision: Option<u64>,
) -> u64 {
    // Camera setters may run more than once before one trace frame presents.
    // Exact publication must expose the renderer's committed frame identity,
    // not the compatibility counter for those unpublished mutations.
    match renderer_revision {
        Some(renderer_revision) => renderer_revision,
        None => session_revision,
    }
}

impl SurfaceRenderSession {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        renderer: Renderer,
        presenter: SurfacePresenter,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        Self::new_with_surface_owner(renderer, SessionSurfaceOwner::standalone(presenter), camera)
    }

    /// Creates a native window session. Exact Packed construction allocates
    /// only the Surface host before the renderer publishes its sole GPU graph;
    /// Direct and Paged retain the standalone presenter route.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_window<T>(
        mut renderer: Renderer,
        target: T,
        width: u32,
        height: u32,
        camera: Camera,
    ) -> Result<Self, RendererError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let presenter = SessionSurfaceOwner::from_window(&renderer, target, width, height).await?;
        let (surface_width, surface_height) = presenter.surface_size();
        renderer.set_size(surface_width, surface_height)?;
        Self::new_with_surface_owner(renderer, presenter, camera)
    }

    /// Creates a native raw-handle session without changing the embedding ABI.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that both raw handles remain valid until the
    /// returned session is dropped.
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn from_raw_handles(
        mut renderer: Renderer,
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        let presenter = unsafe {
            SessionSurfaceOwner::from_raw_handles(
                &renderer,
                raw_display_handle,
                raw_window_handle,
                width,
                height,
            )?
        };
        let (surface_width, surface_height) = presenter.surface_size();
        renderer.set_size(surface_width, surface_height)?;
        Self::new_with_surface_owner(renderer, presenter, camera)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn new_with_surface_owner(
        mut renderer: Renderer,
        presenter: SessionSurfaceOwner,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if !renderer.has_scene() {
            return Err(RendererError::SceneNotLoaded);
        }
        if renderer.geometry_path() != presenter.geometry_path() {
            return Err(RendererError::InvalidConfig);
        }
        let schedule = SessionSchedule::new(&renderer, camera)?;
        // Prepare the only eagerly allocated session-owned collection before
        // staging release. Nothing after the handoff below allocates or can
        // fail before the completed session value is returned.
        let evidence = SessionEvidence::new();

        // Native Packed activates one complete Exact Scene/PlanSet/Raster
        // candidate against the presenter's existing device and queue. Only
        // the final renderer publication releases upload-only source planes.
        // Direct, Paged and Web retain the established compatibility route.
        #[cfg(not(target_arch = "wasm32"))]
        let exact_plan_receipt = if presenter.geometry_path() == GeometryPath::PackedAtlas {
            let (device, queue, format, indirect_execution_supported) =
                presenter.exact_runtime_context();
            let mut candidate = pollster::block_on(renderer.prepare_surface_exact_candidate(
                &device,
                &queue,
                format,
                indirect_execution_supported,
            ))?;
            let (surface_width, surface_height) = presenter.surface_size();
            candidate.seed_surface_frame_baseline(
                camera,
                crate::renderer::frame::Viewport::new(surface_width, surface_height)
                    .expect("configured Surface viewport"),
            );
            renderer.publish_surface_exact_candidate(candidate)?;
            Some(ExactSurfacePlanState::CpuPostSort)
        } else {
            renderer.finish_surface_upload_handoff(presenter.geometry_path())?;
            None
        };
        #[cfg(target_arch = "wasm32")]
        renderer.finish_surface_upload_handoff(presenter.geometry_path())?;
        #[cfg(not(target_arch = "wasm32"))]
        let legacy_stats_availability = if exact_plan_receipt.is_some() {
            LegacySurfaceStatsAvailability::Unavailable
        } else {
            LegacySurfaceStatsAvailability::Current
        };
        Ok(Self {
            renderer,
            presenter,
            exact_plan_receipt,
            exact_order_generation_receipt: None,
            current_stats_submission: SurfaceCurrentStatsSubmission::NotRequested,
            legacy_stats_availability,
            camera,
            schedule,
            order_backend: SurfaceOrderBackend::Cpu,
            projected_draw_policy: SurfaceProjectedDrawPolicy::Adaptive,
            exact_projected_draw_policy_requested: exact_plan_receipt
                .map(|_| SurfaceProjectedDrawPolicy::Adaptive),
            gpu_producer_measurement: SurfaceGpuProducerMeasurementControl::default(),
            presented_order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_order_initialized: false,
            adaptive_policy: AdaptiveOrderPolicy::default(),
            adaptive_projected_cpu: AdaptiveProjectedDrawPolicy::default(),
            adaptive_projected_gpu: AdaptiveProjectedDrawPolicy::default(),
            adaptive_probe_owner: None,
            blocked_order_choice: None,
            adaptive_gpu_failure: None,
            pending_order_backend: None,
            pending_adaptive_choice: None,
            pending_projected_choice: None,
            camera_revision: 0,
            last_stats: FrameStats::zero(),
            latest_cpu_order_measurement: None,
            latest_gpu_order_measurement: None,
            evidence,
            pending_async_completion: None,
        })
    }

    /// Browser construction is asynchronous because WebGPU validation and OOM
    /// scopes must settle without blocking the browser event loop. Packed
    /// sessions publish the same single Exact runtime used by native Surface
    /// consumers; Direct and Paged retain their compatibility routes.
    #[cfg(target_arch = "wasm32")]
    pub async fn new(
        renderer: Renderer,
        presenter: SurfacePresenter,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        Self::new_with_surface_owner(renderer, SessionSurfaceOwner::standalone(presenter), camera)
            .await
    }

    /// Creates a browser canvas session. Packed selects the renderer-owned
    /// Exact graph and retains only the crate-private Surface host.
    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(
        mut renderer: Renderer,
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        let presenter = SessionSurfaceOwner::from_canvas(&renderer, canvas, width, height).await?;
        let (surface_width, surface_height) = presenter.surface_size();
        renderer.set_size(surface_width, surface_height)?;
        Self::new_with_surface_owner(renderer, presenter, camera).await
    }

    #[cfg(target_arch = "wasm32")]
    async fn new_with_surface_owner(
        mut renderer: Renderer,
        presenter: SessionSurfaceOwner,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if !renderer.has_scene() {
            return Err(RendererError::SceneNotLoaded);
        }
        if renderer.geometry_path() != presenter.geometry_path() {
            return Err(RendererError::InvalidConfig);
        }
        let evidence = SessionEvidence::new();
        let schedule = SessionSchedule::new(&renderer, camera)?;
        let exact_plan_receipt = if presenter.geometry_path() == GeometryPath::PackedAtlas {
            let (device, queue, format, indirect_execution_supported) =
                presenter.exact_runtime_context();
            let mut candidate = renderer
                .prepare_surface_exact_candidate(
                    &device,
                    &queue,
                    format,
                    indirect_execution_supported,
                )
                .await?;
            let (surface_width, surface_height) = presenter.surface_size();
            candidate.seed_surface_frame_baseline(
                camera,
                crate::renderer::frame::Viewport::new(surface_width, surface_height)
                    .expect("configured Surface viewport"),
            );
            renderer.publish_surface_exact_candidate(candidate)?;
            Some(ExactSurfacePlanState::CpuPostSort)
        } else {
            renderer.finish_surface_upload_handoff(presenter.geometry_path())?;
            None
        };
        let legacy_stats_availability = if exact_plan_receipt.is_some() {
            LegacySurfaceStatsAvailability::Unavailable
        } else {
            LegacySurfaceStatsAvailability::Current
        };
        Ok(Self {
            renderer,
            presenter,
            exact_plan_receipt,
            exact_order_generation_receipt: None,
            current_stats_submission: SurfaceCurrentStatsSubmission::NotRequested,
            legacy_stats_availability,
            camera,
            schedule,
            order_backend: SurfaceOrderBackend::Cpu,
            projected_draw_policy: SurfaceProjectedDrawPolicy::Adaptive,
            exact_projected_draw_policy_requested: exact_plan_receipt
                .map(|_| SurfaceProjectedDrawPolicy::Adaptive),
            gpu_producer_measurement: SurfaceGpuProducerMeasurementControl::default(),
            presented_order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_order_initialized: false,
            adaptive_policy: AdaptiveOrderPolicy::default(),
            adaptive_projected_cpu: AdaptiveProjectedDrawPolicy::default(),
            adaptive_projected_gpu: AdaptiveProjectedDrawPolicy::default(),
            adaptive_probe_owner: None,
            blocked_order_choice: None,
            adaptive_gpu_failure: None,
            pending_order_backend: None,
            pending_adaptive_choice: None,
            pending_projected_choice: None,
            camera_revision: 0,
            last_stats: FrameStats::zero(),
            latest_cpu_order_measurement: None,
            latest_gpu_order_measurement: None,
            evidence,
        })
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Physical adapter identity selected for this session's Surface.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        self.presenter.adapter_info()
    }

    /// Number of source records addressable by this session's selected path.
    pub fn addressable_splat_count(&self) -> usize {
        self.presenter.addressable_splat_count()
    }

    pub fn adapter_max_storage_buffers_per_shader_stage(&self) -> u32 {
        self.presenter
            .adapter_max_storage_buffers_per_shader_stage()
    }

    pub fn adapter_max_storage_buffer_binding_size(&self) -> u64 {
        self.presenter.adapter_max_storage_buffer_binding_size()
    }

    fn exact_plan_state(&self) -> Option<ExactSurfacePlanState> {
        let policy = self.renderer.exact_surface_policy()?;
        let state = ExactSurfacePlanState::from_policy(policy);
        debug_assert_eq!(self.exact_plan_receipt, Some(state));
        Some(state)
    }

    fn commit_exact_plan_state(
        &mut self,
        state: ExactSurfacePlanState,
    ) -> Result<(), RendererError> {
        commit_exact_plan_state(
            &mut self.renderer,
            &mut self.exact_plan_receipt,
            &mut self.order_backend,
            &mut self.projected_draw_policy,
            state,
        )
    }

    fn try_set_exact_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<bool, RendererError> {
        let Some(current) = self.exact_plan_state() else {
            return Ok(false);
        };
        self.commit_exact_plan_state(current.with_producer(producer)?)?;
        Ok(true)
    }

    /// Requests one current-stats receipt from the next eligible Exact
    /// Surface frame. Direct, Paged and Web retain the additive fail-closed
    /// compatibility result until their later consumer cutovers.
    pub fn request_current_stats(&mut self) -> SurfaceCurrentStatsRequest {
        if self.exact_plan_state().is_some() {
            return self
                .renderer
                .request_exact_surface_current_stats()
                .map(Into::into)
                .unwrap_or(SurfaceCurrentStatsRequest::Unsampled(
                    crate::SurfaceCurrentStatsUnsampledReason::GpuUnavailable,
                ));
        }
        legacy_surface_current_stats_request(&mut self.renderer)
    }

    /// Returns the current frame's presentation-committed current-stats
    /// submission. The legacy Surface path never issues a ticket; M2a may back
    /// this additive getter with session-private state updated only after a
    /// successful present.
    pub const fn current_stats_submission(&self) -> SurfaceCurrentStatsSubmission {
        self.current_stats_submission
    }

    /// Polls at most one current-stats resolution or atomic terminal. The
    /// Renderer retains any additional ready terminals in its bounded queue.
    pub fn poll_current_stats(&mut self) -> SurfaceCurrentStatsPoll {
        if self.exact_plan_state().is_some() {
            let poll = self
                .renderer
                .poll_exact_surface_current_stats()
                .map(Into::into)
                .unwrap_or(SurfaceCurrentStatsPoll::Empty);
            if let SurfaceCurrentStatsPoll::Terminal(terminal) = poll {
                self.evidence.publish_exact_order_terminal(terminal);
            }
            self.legacy_stats_availability.observe_poll(
                self.current_stats_submission,
                poll,
                &mut self.last_stats,
            );
            return poll;
        }
        legacy_surface_current_stats_poll(&mut self.renderer)
    }

    /// Requests an exact readback of the next native Surface frame. This is a
    /// diagnostic operation: it may reconfigure the swapchain for `COPY_SRC`,
    /// but ordinary sessions never pay that cost unless explicitly requested.
    /// Resize and a second request fail while this capture remains armed. If a
    /// frame cannot be presented, retry rendering or call
    /// [`Self::cancel_surface_capture`] before resize/re-request.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_surface_capture(&mut self) -> Result<(), RendererError> {
        self.presenter.request_surface_capture()?;
        Ok(())
    }

    /// Cancels a requested native Surface capture and releases its readback
    /// buffer. This also discards a presented capture that was not taken.
    /// Returns false when no capture was armed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_surface_capture(&mut self) -> bool {
        self.presenter.cancel_surface_capture()
    }

    /// Blocks until the requested presented frame is readable and returns
    /// canonical RGBA8 bytes. Calling before a frame presents fails closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_surface_capture(&mut self) -> Result<SurfaceFrameCapture, RendererError> {
        Ok(self.presenter.take_surface_capture()?)
    }

    pub fn geometry_path(&self) -> GeometryPath {
        self.renderer.geometry_path()
    }

    pub fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        if self.exact_plan_receipt.is_some() {
            return SurfaceRasterExecutionPlan::ProjectedQuadsExact;
        }
        self.presenter.raster_execution_plan()
    }

    pub const fn projected_draw_policy(&self) -> SurfaceProjectedDrawPolicy {
        if let Some(requested) = self.exact_projected_draw_policy_requested {
            return requested;
        }
        self.projected_draw_policy
    }

    pub const fn gpu_order_producer(&self) -> SurfaceGpuOrderProducer {
        if let Some(state) = self.exact_plan_receipt {
            return state.producer();
        }
        self.presenter.gpu_order_producer()
    }

    /// Prepares a complete dormant producer graph without changing the
    /// selected producer or scheduling state. This is the browser-safe first
    /// half of the transactional A/B switch.
    pub async fn prepare_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        if self.exact_plan_state().is_some() {
            // Exact construction already published the complete set of plans
            // the adapter can execute. A CPU-only Surface must not turn the
            // existence of that runtime into a false GPU-prepared receipt.
            return prepare_exact_gpu_order_producer(&self.renderer, producer);
        }
        if producer == SurfaceGpuOrderProducer::Preproject
            && !gpu_producer_measurement_context_is_valid(
                self.geometry_path(),
                self.raster_execution_plan(),
                self.projected_draw_policy,
            )
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        self.presenter.prepare_gpu_order_producer(producer).await?;
        Ok(())
    }

    /// Transactionally selects the Packed GPU producer. The CPU lane and the
    /// outer CPU/GPU/Adaptive policy remain unchanged.
    pub fn set_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        if self.try_set_exact_gpu_order_producer(producer)? {
            return Ok(());
        }
        if !validate_gpu_order_producer_transition(
            self.gpu_order_producer(),
            producer,
            self.geometry_path(),
            self.raster_execution_plan(),
            self.projected_draw_policy,
        )? {
            return Ok(());
        }
        self.presenter.set_gpu_order_producer(producer)?;
        self.finish_gpu_order_producer_transition();
        Ok(())
    }

    /// Browser-safe all-or-nothing producer switch. The complete candidate is
    /// scoped and published before the infallible selector/state transition.
    #[cfg(target_arch = "wasm32")]
    pub async fn set_gpu_order_producer_async(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        if self.try_set_exact_gpu_order_producer(producer)? {
            return Ok(());
        }
        if !validate_gpu_order_producer_transition(
            self.gpu_order_producer(),
            producer,
            self.geometry_path(),
            self.raster_execution_plan(),
            self.projected_draw_policy,
        )? {
            return Ok(());
        }
        self.presenter.prepare_gpu_order_producer(producer).await?;
        self.presenter.set_gpu_order_producer(producer)?;
        self.finish_gpu_order_producer_transition();
        Ok(())
    }

    fn finish_gpu_order_producer_transition(&mut self) {
        self.gpu_order_initialized = false;
        self.pending_order_backend = None;
        self.pending_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.latest_gpu_order_measurement = None;
        self.reset_adaptive_policy();
        self.adaptive_projected_cpu.suspend_learning();
        self.adaptive_projected_gpu.suspend_learning();
        self.adaptive_probe_owner = None;
        self.blocked_order_choice = None;
        self.schedule.force_sort();
    }

    /// Enables the independent per-GPU-frame producer receipt ring. It is a
    /// diagnostic control and is admitted only under forced Compact so no
    /// Phase1 Candidate/Adaptive ticket can share the experiment.
    pub fn set_gpu_producer_measurement_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            let next = current.with_gpu_producer_measurement(enabled)?;
            debug_assert_eq!(next, current);
            return Ok(());
        }
        let context_is_valid = gpu_producer_measurement_context_is_valid(
            self.geometry_path(),
            self.raster_execution_plan(),
            self.projected_draw_policy,
        );
        if !self
            .gpu_producer_measurement
            .transition(enabled, context_is_valid)?
        {
            return Ok(());
        }
        self.pending_order_backend = None;
        self.pending_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.latest_gpu_order_measurement = None;
        reset_adaptive_for_gpu_producer_measurement_transition(
            &mut self.adaptive_policy,
            &mut self.adaptive_projected_cpu,
            &mut self.adaptive_projected_gpu,
            &mut self.adaptive_probe_owner,
            &mut self.blocked_order_choice,
        );
        self.schedule.force_sort();
        Ok(())
    }

    /// Reports the renderer-owned producer-measurement state. This is the
    /// authoritative read-only query for integration layers.
    pub fn gpu_producer_measurement_enabled(&self) -> bool {
        self.gpu_producer_measurement.enabled()
    }

    /// Transactionally changes only the projected draw strategy. A rejected
    /// forced Compact request leaves the previous policy and learned lanes
    /// untouched; repeated requests are no-ops.
    pub fn set_projected_draw_policy(
        &mut self,
        policy: SurfaceProjectedDrawPolicy,
    ) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            self.commit_exact_plan_state(current.with_projected_policy(policy)?)?;
            self.exact_projected_draw_policy_requested = Some(policy);
            return Ok(());
        }
        if policy != SurfaceProjectedDrawPolicy::Compact
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement.enabled())
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        if !commit_projected_draw_policy_transition(
            &mut self.projected_draw_policy,
            policy,
            self.presenter.projected_contributor_indirect_draw_enabled(),
            &mut self.adaptive_policy,
            &mut self.adaptive_projected_cpu,
            &mut self.adaptive_projected_gpu,
            &mut self.adaptive_probe_owner,
            &mut self.blocked_order_choice,
            &mut self.pending_order_backend,
            &mut self.pending_adaptive_choice,
            &mut self.pending_projected_choice,
        )? {
            return Ok(());
        }
        self.schedule.force_sort();
        Ok(())
    }

    /// Selects the exact Packed raster implementation without changing the
    /// CPU/GPU/Adaptive ordering policy or the source scene contract.
    pub fn set_raster_execution_plan(
        &mut self,
        plan: SurfaceRasterExecutionPlan,
    ) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            let next = current.with_raster(plan)?;
            debug_assert_eq!(next, current);
            return Ok(());
        }
        // A repeated setter call is not a strategy transition. In particular,
        // do not erase Adaptive's measured history or force a redundant sort
        // when bindings re-apply their current configuration.
        let previous = self.presenter.raster_execution_plan();
        if previous == plan {
            return Ok(());
        }
        if plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement.enabled())
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        self.presenter.set_raster_execution_plan(plan)?;
        self.pending_order_backend = None;
        self.pending_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        // Projection/raster queue pressure is part of the FrameCompletion
        // metric. Measurements learned under the previous raster plan are not
        // comparable, even though both plans consume the same exact order.
        let adaptive_reset =
            reset_adaptive_for_raster_transition(&mut self.adaptive_policy, previous, plan);
        debug_assert!(adaptive_reset);
        self.reset_projected_draw_policies();
        self.schedule.force_sort();
        Ok(())
    }

    /// Native compatibility setter for the experimental geometry A/B knob.
    ///
    /// Native Direct/Paged switches remain transactional, while transitions
    /// entering or leaving Packed are rejected before mutation. On Web,
    /// geometry is constructor-only: same-path calls are idempotent and every
    /// changed-path request returns Unsupported.
    pub fn set_geometry_path(&mut self, path: GeometryPath) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            let next = current.with_geometry(path)?;
            debug_assert_eq!(next, current);
            return Ok(());
        }
        if path != GeometryPath::PackedAtlas
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement.enabled())
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        match surface_geometry_switch_entry(
            cfg!(target_arch = "wasm32"),
            self.geometry_path(),
            path,
        ) {
            SurfaceGeometrySwitchEntry::AlreadyActive => return Ok(()),
            SurfaceGeometrySwitchEntry::Unsupported => {
                return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported.into());
            }
            SurfaceGeometrySwitchEntry::Synchronous => {}
        }
        if path == GeometryPath::PagedActiveAtlas && self.order_backend != SurfaceOrderBackend::Cpu
        {
            return Err(RendererError::InvalidConfig);
        }
        let changed = try_switch_renderer_geometry_path(&mut self.renderer, path, |renderer| {
            self.presenter.set_geometry_path(path, renderer)
        })?;
        if !changed {
            return Ok(());
        }
        self.finish_geometry_path_switch(path);
        Ok(())
    }

    /// Browser compatibility shim for the historical async setter shape.
    ///
    /// Geometry is constructor-only on Web. This returns success only for the
    /// active path and returns Unsupported for every changed-path request,
    /// without preparing or publishing renderer or presenter candidates.
    #[cfg(target_arch = "wasm32")]
    pub async fn set_geometry_path_async(
        &mut self,
        path: GeometryPath,
    ) -> Result<(), RendererError> {
        self.set_geometry_path(path)
    }

    fn finish_geometry_path_switch(&mut self, _path: GeometryPath) {
        #[cfg(not(target_arch = "wasm32"))]
        if _path == GeometryPath::PagedActiveAtlas {
            self.disable_async_sort();
        }
        self.gpu_order_initialized = false;
        self.pending_order_backend = None;
        self.pending_adaptive_choice = None;
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        self.reset_adaptive_policy();
        self.reset_projected_draw_policies();
        self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
        self.schedule.force_sort();
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Monotonic identity of the camera state currently owned by this session.
    ///
    /// Native benchmark receipts use this read-only value to prove that the
    /// pose/intrinsics they report belong to the same revision that was
    /// submitted and presented. Camera scheduling remains owned by the shared
    /// Surface session.
    pub const fn camera_revision(&self) -> u64 {
        self.camera_revision
    }

    pub fn set_camera(&mut self, camera: Camera) -> Result<(), RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if self.camera != camera {
            self.camera = camera;
            self.camera_revision = self.camera_revision.wrapping_add(1);
            if self.exact_plan_receipt.is_some() {
                return Ok(());
            }
            self.schedule.mark_camera_changed();
        }
        Ok(())
    }

    pub fn surface_size(&self) -> (u32, u32) {
        self.presenter.surface_size()
    }

    /// Actual raster target dimensions before any Surface presentation.
    pub fn internal_render_size(&self) -> (u32, u32) {
        if self.exact_plan_receipt.is_some() {
            return self.presenter.surface_size();
        }
        self.presenter.internal_render_size()
    }

    pub fn last_presented_size(&self) -> Option<(u32, u32)> {
        self.presenter.last_presented_size()
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let previous_size = self.presenter.surface_size();
        self.presenter.resize(width, height)?;
        let (surface_width, surface_height) = self.presenter.surface_size();
        self.renderer.set_size(surface_width, surface_height)?;
        if previous_size != (surface_width, surface_height) {
            if self.exact_plan_receipt.is_some() {
                return Ok(());
            }
            self.pending_order_backend = None;
            self.pending_adaptive_choice = None;
            self.latest_gpu_order_measurement = None;
            self.latest_cpu_order_measurement = None;
            self.reset_adaptive_policy();
            self.reset_projected_draw_policies();
            self.schedule.force_sort();
        }
        Ok(())
    }

    /// Browser-only transactional resize for the production Packed +
    /// Projected Surface. No renderer/session size or scheduling state is
    /// published until the presenter's async configure transaction succeeds.
    #[cfg(target_arch = "wasm32")]
    pub async fn resize_async(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let candidate_config = gsplat_core::RendererConfig {
            width,
            height,
            ..self.renderer.config()
        };
        candidate_config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;
        let previous_size = self.presenter.surface_size();
        self.presenter.resize_async(width, height).await?;
        // On wasm, `Renderer::set_size` performs exactly the validation above
        // and then publishes the copyable config; it creates no GPU resource.
        self.renderer.set_size(width, height)?;
        let (surface_width, surface_height) = self.presenter.surface_size();
        if previous_size != (surface_width, surface_height) {
            if self.exact_plan_receipt.is_some() {
                return Ok(());
            }
            self.pending_order_backend = None;
            self.pending_adaptive_choice = None;
            self.latest_gpu_order_measurement = None;
            self.latest_cpu_order_measurement = None;
            self.reset_adaptive_policy();
            self.reset_projected_draw_policies();
            self.schedule.force_sort();
        }
        Ok(())
    }

    pub fn sort_interval(&self) -> u32 {
        self.schedule.sort_interval()
    }

    pub const fn order_backend(&self) -> SurfaceOrderBackend {
        if let Some(state) = self.exact_plan_receipt {
            return state.order_backend();
        }
        self.order_backend
    }

    /// Transactionally prepares the complete GPU-order resource graph without
    /// changing the selected backend, frame state, or Adaptive evidence.
    /// Browser callers must await this before selecting GPU or Adaptive.
    pub async fn prepare_gpu_order(&mut self) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            return prepare_exact_gpu_order(&self.renderer, current);
        }
        self.presenter.prepare_gpu_order().await?;
        Ok(())
    }

    pub fn set_order_backend(&mut self, backend: SurfaceOrderBackend) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            return self.commit_exact_plan_state(current.with_order_backend(backend));
        }
        if self.order_backend == backend {
            return Ok(());
        }
        if backend != SurfaceOrderBackend::Cpu
            && self.geometry_path() == GeometryPath::PagedActiveAtlas
        {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if backend != SurfaceOrderBackend::Cpu && self.schedule.configured_async_enabled() {
            return Err(RendererError::InvalidConfig);
        }
        let gpu_prepare_error = if backend == SurfaceOrderBackend::Cpu {
            None
        } else {
            self.presenter.prepare_direct_gpu_order().err()
        };
        let gpu_prepare_failed = match (backend, gpu_prepare_error) {
            (SurfaceOrderBackend::Gpu, Some(error)) => return Err(error.into()),
            (SurfaceOrderBackend::Adaptive, Some(error)) => {
                let error = RendererError::from(error);
                if let Some(reason) = adaptive_gpu_order_failure_reason(&error) {
                    self.adaptive_gpu_failure = Some(reason);
                    true
                } else {
                    return Err(error);
                }
            }
            _ => false,
        };
        self.order_backend = backend;
        self.pending_order_backend = None;
        self.pending_adaptive_choice = None;
        self.pending_projected_choice = None;
        if backend != SurfaceOrderBackend::Adaptive || !gpu_prepare_failed {
            self.adaptive_gpu_failure = None;
        }
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        self.reset_adaptive_policy();
        // Backend-specific projected lanes keep their completed history, but
        // neither lane may retain an in-flight ticket or arbitration owner
        // across a backend transition. An eventual old receipt is still
        // exposed in diagnostics and cannot mutate either policy.
        self.adaptive_projected_cpu.suspend_learning();
        self.adaptive_projected_gpu.suspend_learning();
        self.adaptive_probe_owner = None;
        self.blocked_order_choice = None;
        if backend == SurfaceOrderBackend::Adaptive && gpu_prepare_failed {
            self.adaptive_policy.gpu_failed();
        }
        self.schedule.force_sort();
        Ok(())
    }

    pub fn sort_schedule(&self) -> SurfaceSortSchedule {
        if self.exact_plan_receipt.is_some() {
            return SurfaceSortSchedule::Interval(1);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self
            .schedule
            .async_enabled(self.geometry_path(), self.order_backend)
        {
            return SurfaceSortSchedule::AsyncLatest {
                interval: self.schedule.sort_interval(),
            };
        }
        SurfaceSortSchedule::Interval(self.schedule.sort_interval())
    }

    pub fn set_sort_schedule(
        &mut self,
        schedule: SurfaceSortSchedule,
    ) -> Result<(), RendererError> {
        if schedule.interval() == 0 {
            return Err(RendererError::InvalidConfig);
        }
        if let Some(current) = self.exact_plan_state() {
            let next = current.with_sort_schedule(schedule)?;
            debug_assert_eq!(next, current);
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. }) {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. })
            && !async_sort_supported(self.geometry_path(), self.order_backend)
        {
            return Err(RendererError::InvalidConfig);
        }
        match schedule {
            SurfaceSortSchedule::Interval(interval) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.set_async_sort_enabled(false)?;
                self.set_sort_interval(interval)
            }
            SurfaceSortSchedule::AsyncLatest { interval } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.set_async_sort_enabled(true)?;
                    self.set_sort_interval(interval)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = interval;
                    unreachable!("async schedules are rejected before mutation")
                }
            }
        }
    }

    pub fn set_sort_interval(&mut self, interval: u32) -> Result<(), RendererError> {
        if interval == 0 {
            return Err(RendererError::InvalidConfig);
        }
        if let Some(current) = self.exact_plan_state() {
            let next = current.with_sort_interval(interval)?;
            debug_assert_eq!(next, current);
            return Ok(());
        }
        if self.schedule.set_sort_interval(interval) {
            self.reset_adaptive_policy();
        }
        Ok(())
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        self.presenter.set_frame_latency(latency);
        if self.exact_plan_receipt.is_some() {
            self.renderer.reset_exact_surface_performance_learning();
        }
        // Preserve the established setter contract: every invocation resets
        // latency-dependent learning, including a repeated/clamped value for
        // which Surface configuration itself is already a no-op.
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        self.reset_adaptive_policy();
        self.reset_projected_draw_policies();
        self.schedule.force_sort();
    }

    fn reset_adaptive_policy(&mut self) {
        // Select the backend on the user-visible frame boundary. A GPU radix
        // timestamp can beat CPU preprocess+sort while still reducing total
        // throughput by contending with projection and raster work on the same
        // queue. The paired ABBA completion probe keeps one incumbent while a
        // formal receipt is pending, so both backends include their real queue
        // pressure without turning readback latency into challenger residency.
        self.adaptive_policy.reset(adaptive_primary_metric());
        if self.adaptive_probe_owner == Some(AdaptiveProbeOwner::Order) {
            self.adaptive_probe_owner = None;
        }
        self.blocked_order_choice = None;
    }

    fn reset_projected_draw_policies(&mut self) {
        self.adaptive_projected_cpu.reset();
        self.adaptive_projected_gpu.reset();
        self.adaptive_probe_owner = None;
        self.pending_projected_choice = None;
        self.blocked_order_choice = None;
    }

    fn projected_policy(&self, backend: SurfaceOrderBackendUsed) -> &AdaptiveProjectedDrawPolicy {
        match backend {
            SurfaceOrderBackendUsed::Cpu => &self.adaptive_projected_cpu,
            SurfaceOrderBackendUsed::Gpu => &self.adaptive_projected_gpu,
        }
    }

    fn projected_policy_mut(
        &mut self,
        backend: SurfaceOrderBackendUsed,
    ) -> &mut AdaptiveProjectedDrawPolicy {
        match backend {
            SurfaceOrderBackendUsed::Cpu => &mut self.adaptive_projected_cpu,
            SurfaceOrderBackendUsed::Gpu => &mut self.adaptive_projected_gpu,
        }
    }

    fn refresh_adaptive_probe_owner(&mut self) {
        let finished = match self.adaptive_probe_owner {
            Some(AdaptiveProbeOwner::Order) => !self.adaptive_policy.cohort_active(),
            Some(AdaptiveProbeOwner::ProjectedCpu) => !self.adaptive_projected_cpu.cohort_active(),
            Some(AdaptiveProbeOwner::ProjectedGpu) => !self.adaptive_projected_gpu.cohort_active(),
            None => false,
        };
        if finished {
            let projected_owner_finished = matches!(
                self.adaptive_probe_owner,
                Some(AdaptiveProbeOwner::ProjectedCpu | AdaptiveProbeOwner::ProjectedGpu)
            );
            let projected_incumbent_changed = match self.adaptive_probe_owner {
                Some(AdaptiveProbeOwner::ProjectedCpu) => {
                    self.adaptive_projected_cpu.take_incumbent_changed()
                }
                Some(AdaptiveProbeOwner::ProjectedGpu) => {
                    self.adaptive_projected_gpu.take_incumbent_changed()
                }
                Some(AdaptiveProbeOwner::Order) | None => false,
            };
            if should_reset_order_for_projected_incumbent_change(
                self.order_backend,
                self.projected_draw_policy,
                self.adaptive_policy.metric(),
                self.adaptive_policy.has_pending_sample(),
                projected_owner_finished,
                projected_incumbent_changed,
            ) {
                // FrameCompletion includes raster queue pressure. Publish the
                // projected winner first, then discard only order evidence at
                // this owner boundary; neither projected lane is reset.
                self.adaptive_policy.reset(adaptive_primary_metric());
                self.blocked_order_choice = None;
                self.schedule.force_sort();
            }
            if projected_owner_finished && self.blocked_order_choice.is_some() {
                self.schedule.force_sort();
            }
            self.adaptive_probe_owner = None;
        }
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    /// Returns the legacy FrameStats projection only while its V/D counts are
    /// demonstrably current for the last successfully presented Surface frame.
    pub fn legacy_stats(&self) -> Option<FrameStats> {
        self.legacy_stats_availability.get(self.last_stats)
    }

    /// Formal Adaptive sample awaiting its asynchronous terminal receipt.
    pub fn adaptive_pending_sample(&self) -> Option<SurfaceAdaptivePendingSample> {
        if self.exact_plan_state().is_some() {
            // M2a exposes no legacy benchmark ticket for the renderer-owned
            // mandatory whole-plan sampler.
            return None;
        }
        (self.order_backend == SurfaceOrderBackend::Adaptive)
            .then(|| self.adaptive_policy.pending_sample())
            .flatten()
    }

    /// Current Adaptive policy state, including transitions completed by an
    /// explicit receipt poll between rendered frames.
    pub fn adaptive_state(&self) -> SurfaceAdaptiveState {
        if self.exact_plan_state().is_some() {
            return match self.renderer.exact_surface_adaptive_state() {
                Some(ExactAdaptivePolicyState::CpuLearning) => SurfaceAdaptiveState::CpuLearning,
                Some(ExactAdaptivePolicyState::CpuStable) => SurfaceAdaptiveState::CpuStable,
                Some(ExactAdaptivePolicyState::GpuStable) => SurfaceAdaptiveState::GpuStable,
                Some(ExactAdaptivePolicyState::GpuProbe) => SurfaceAdaptiveState::GpuProbe,
                Some(ExactAdaptivePolicyState::CpuProbe) => SurfaceAdaptiveState::CpuProbe,
                Some(ExactAdaptivePolicyState::Disabled) | None => SurfaceAdaptiveState::Disabled,
            };
        }
        if self.order_backend == SurfaceOrderBackend::Adaptive {
            self.adaptive_policy.state()
        } else {
            SurfaceAdaptiveState::Disabled
        }
    }

    pub fn projected_draw_adaptive_state(
        &self,
        backend: SurfaceOrderBackendUsed,
    ) -> SurfaceProjectedDrawAdaptiveState {
        if self.exact_plan_state().is_some() {
            // Exact Adaptive is a whole-plan controller. It deliberately has
            // no independent projected learner to report.
            let _ = backend;
            return SurfaceProjectedDrawAdaptiveState::Disabled;
        }
        if self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive
            && self.raster_execution_plan() == SurfaceRasterExecutionPlan::ProjectedQuadsExact
        {
            self.projected_policy(backend).state()
        } else {
            SurfaceProjectedDrawAdaptiveState::Disabled
        }
    }

    pub fn projected_draw_adaptive_pending_sample(
        &self,
        backend: SurfaceOrderBackendUsed,
    ) -> Option<SurfaceProjectedDrawAdaptivePendingSample> {
        if self.exact_plan_state().is_some() {
            let _ = backend;
            return None;
        }
        (self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive)
            .then(|| self.projected_policy(backend).pending_sample())
            .flatten()
    }

    /// Polls CPU/GPU completion callbacks and publishes terminal receipts
    /// without acquiring a Surface texture, encoding a draw, or submitting
    /// more queue work. Benchmarks use this to isolate one formal sample from
    /// artificial drain-frame backlog.
    pub fn poll_order_measurement_receipts(&mut self) {
        if self.exact_plan_state().is_some() {
            return;
        }
        let _ = self.collect_order_measurements();
        let _ = self.collect_projected_draw_measurements();
        let _ = self.collect_gpu_producer_measurements();
    }

    /// Boundedly advances callbacks for already-submitted Surface queue work.
    ///
    /// This terminalization path never acquires a Surface texture, encodes or
    /// submits commands, or requests a measurement, so it cannot issue a new
    /// benchmark ticket. `Ok(false)` means the finite wait elapsed while work
    /// remained pending; callers decide their own total drain bound.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pump_receipts(&mut self, timeout: Duration) -> Result<bool, RendererError> {
        if self.exact_plan_state().is_some() {
            return self
                .renderer
                .exact_runtime_mut()?
                .pump_receipt_callbacks(timeout);
        }
        let completed = self.presenter.pump_receipt_callbacks(timeout);
        self.poll_order_measurement_receipts();
        completed
    }

    /// Returns the last successful render call's immutable submission identity
    /// for one legacy compatibility channel.
    pub fn compatibility_submission(
        &self,
        channel: SurfaceCompatibilityChannel,
    ) -> Option<SurfaceCompatibilitySubmission> {
        self.evidence.submission(channel)
    }

    /// Drains one selected terminal without blocking. A terminal already in
    /// the selected queue is consumed before completion callbacks may advance.
    pub fn poll_compatibility_terminal(
        &mut self,
        selector: SurfaceCompatibilityTerminalSelector,
    ) -> SurfaceCompatibilityTerminalPoll {
        if self.evidence.terminal_is_empty(selector) {
            self.poll_order_measurement_receipts();
        }
        self.evidence.poll_terminal(selector)
    }

    /// Takes one successful order/projected V/C/D receipt by non-zero ticket.
    /// Counts remain independent of terminal-success consumption and are
    /// returned at most once before explicit bounded expiry.
    pub fn take_compatibility_counts(
        &mut self,
        family: SurfaceCompatibilityCountFamily,
        ticket: NonZeroU64,
    ) -> SurfaceCompatibilityCountsTake {
        self.evidence.take_counts(family, ticket)
    }

    /// Drains completed CPU order measurements in ticket order.
    pub fn drain_cpu_order_measurements(&mut self) -> Vec<SurfaceCpuOrderMeasurement> {
        self.evidence.drain_cpu_order()
    }

    /// Drains exact asynchronous GPU timing/count receipts in ticket order.
    pub fn drain_order_measurements(&mut self) -> Vec<SurfaceOrderMeasurement> {
        self.evidence.drain_gpu_order()
    }

    /// Drains terminal failure receipts for issued GPU measurement tickets.
    pub fn drain_order_measurement_failures(&mut self) -> Vec<SurfaceOrderMeasurementFailure> {
        self.evidence.drain_order_failures()
    }

    pub fn drain_projected_draw_measurements(&mut self) -> Vec<SurfaceProjectedDrawMeasurement> {
        self.evidence.drain_projected_successes()
    }

    pub fn drain_projected_draw_measurement_failures(
        &mut self,
    ) -> Vec<SurfaceProjectedDrawMeasurementFailure> {
        self.evidence.drain_projected_failures()
    }

    pub fn drain_gpu_producer_measurements(&mut self) -> Vec<SurfaceGpuProducerMeasurement> {
        self.evidence.drain_producer_successes()
    }

    pub fn drain_gpu_producer_measurement_failures(
        &mut self,
    ) -> Vec<SurfaceGpuProducerMeasurementFailure> {
        self.evidence.drain_producer_failures()
    }

    /// Pops the oldest raw producer success without using the bounded
    /// compatibility view.
    pub fn pop_gpu_producer_measurement(&mut self) -> Option<SurfaceGpuProducerMeasurement> {
        self.evidence.pop_producer_success()
    }

    /// Pops the oldest raw producer failure without using the bounded
    /// compatibility view.
    pub fn pop_gpu_producer_measurement_failure(
        &mut self,
    ) -> Option<SurfaceGpuProducerMeasurementFailure> {
        self.evidence.pop_producer_failure()
    }

    pub fn force_sort_refresh(&mut self) {
        if self.exact_plan_state().is_some() {
            self.renderer
                .request_exact_surface_cpu_refresh()
                .expect("active Exact Surface runtime");
            return;
        }
        self.schedule.force_sort();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_async_sort_enabled(&mut self, enabled: bool) -> Result<(), RendererError> {
        if let Some(current) = self.exact_plan_state() {
            let next = current.with_async_sort(enabled)?;
            debug_assert_eq!(next, current);
            return Ok(());
        }
        if enabled && !async_sort_supported(self.geometry_path(), self.order_backend) {
            return Err(RendererError::InvalidConfig);
        }
        self.schedule.set_async_enabled(&self.renderer, enabled)?;
        if !enabled && self.pending_async_completion.take().is_some() {
            self.schedule.force_sort();
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn disable_async_sort(&mut self) {
        self.schedule.disable_async();
        if self.pending_async_completion.take().is_some() {
            self.schedule.force_sort();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn async_sort_enabled(&self) -> bool {
        self.schedule
            .async_enabled(self.geometry_path(), self.order_backend)
    }

    pub fn render_frame(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        if self.exact_plan_state().is_some() {
            let result = self.render_frame_exact();
            if let Ok(output) = &result {
                self.observe_compatibility_submissions(*output);
            }
            return result;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self
            .schedule
            .async_enabled(self.geometry_path(), self.order_backend)
        {
            let result = self.render_frame_async_sort();
            if let Ok(output) = &result
                && output.frame_presented
            {
                self.observe_compatibility_submissions(*output);
            }
            return result;
        }
        let result = self.render_frame_sync();
        if let Ok(output) = &result {
            self.observe_compatibility_submissions(*output);
        }
        result
    }

    fn observe_compatibility_submissions(&mut self, output: SurfaceFrameOutput) {
        let requested_backend = self.order_backend();
        let requested_producer = self.gpu_order_producer();
        self.evidence.observe_frame_output(
            output,
            requested_backend,
            requested_producer,
            self.gpu_producer_measurement.enabled(),
        );
    }

    fn render_frame_exact(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_started = timer_now();
        let previous_plan = self.renderer.exact_surface_last_plan();
        let force_cpu_order_refresh = self
            .renderer
            .exact_surface_cpu_refresh_requested()
            .expect("active Exact Surface runtime");
        let render_attempt = {
            let runtime = self.renderer.exact_runtime_mut()?;
            SessionFrameExecutor::attempt_exact(
                runtime,
                &mut self.presenter,
                &self.camera,
                force_cpu_order_refresh,
                frame_started,
            )
        };
        let attempt = render_attempt.map_err(map_surface_exact_error)?;
        match attempt.commit(|rendered| self.commit_exact_presented_frame(rendered, frame_started))
        {
            Ok(output) => Ok(output),
            Err(()) => {
                let plan = previous_plan.unwrap_or(PlanId::CpuPostSort);
                Ok(self.exact_surface_output(None, plan, false, frame_started))
            }
        }
    }

    fn commit_exact_presented_frame(
        &mut self,
        rendered: crate::surface::shadow::SurfaceExactFrameResult,
        frame_started: crate::TimerInstant,
    ) -> SurfaceFrameOutput {
        let submission = rendered.submission();
        debug_assert_eq!(
            rendered.target().presentation_sequence(),
            submission.presentation_sequence(),
            "Surface lifecycle and renderer publish the same successful present sequence"
        );
        let presented_current_stats_submission = submission.current_stats_submission().into();
        let plan = submission.plan_id();
        let order_generation = submission.order_generation();
        let order_refreshed =
            exact_order_refreshed(self.exact_order_generation_receipt, plan, order_generation);
        self.schedule
            .record_applied_order(self.camera, submission.frame_identity().camera_revision());
        let output =
            self.exact_surface_output(Some(submission), plan, order_refreshed, frame_started);
        self.exact_order_generation_receipt = Some((plan, order_generation));
        publish_only_on_present(
            &mut self.current_stats_submission,
            Some(presented_current_stats_submission),
        );
        self.last_stats = output.stats;
        self.legacy_stats_availability = LegacySurfaceStatsAvailability::for_presented_frame(
            !output.visible_count_pending,
            self.current_stats_submission,
        );
        self.renderer.publish_exact_surface_stats(output.stats);
        self.presented_order_backend = output.order_backend;
        output
    }

    fn exact_surface_output(
        &self,
        submission: Option<&crate::renderer::GpuFrameSubmission>,
        plan: PlanId,
        order_refreshed: bool,
        frame_started: crate::TimerInstant,
    ) -> SurfaceFrameOutput {
        let frame_presented = submission.is_some();
        let order_backend = match plan {
            PlanId::CpuPostSort => SurfaceOrderBackendUsed::Cpu,
            PlanId::GpuPostSort | PlanId::GpuPreproject => SurfaceOrderBackendUsed::Gpu,
        };
        let projected_draw_execution = match plan {
            PlanId::CpuPostSort | PlanId::GpuPostSort => SurfaceProjectedDrawExecution::Candidate,
            PlanId::GpuPreproject => SurfaceProjectedDrawExecution::Compact,
        };
        let host_timings = submission.and_then(|submission| submission.host_timings());
        let frame_wall_ms = timer_elapsed_ms(frame_started);
        let stats = FrameStats {
            frame_ms: host_timings.map_or(frame_wall_ms, |timings| timings.frame_ms()),
            preprocess_ms: host_timings.map_or(0.0, |timings| timings.preprocess_ms()),
            sort_ms: host_timings.map_or(0.0, |timings| timings.sort_ms()),
            raster_ms: host_timings.map_or(0.0, |timings| timings.raster_ms()),
            visible_count: submission
                .and_then(|submission| submission.visible_count())
                .unwrap_or(0),
            drawn_count: submission
                .and_then(|submission| submission.draw_count())
                .unwrap_or(0),
        };
        let counts_pending = submission.is_some_and(|submission| {
            submission.visible_count().is_none() || submission.draw_count().is_none()
        });
        let order_measurement_submission = if frame_presented && order_refreshed {
            submission
                .and_then(|submission| submission.current_stats_submission().receipt())
                .map_or(SurfaceOrderMeasurementSubmission::NotRequested, |receipt| {
                    SurfaceOrderMeasurementSubmission::Issued {
                        backend: order_backend,
                        ticket: receipt.ticket().get(),
                    }
                })
        } else {
            SurfaceOrderMeasurementSubmission::NotRequested
        };
        let camera_revision = exact_published_camera_revision(
            self.camera_revision,
            submission.map(|submission| submission.frame_identity().camera_revision()),
        );
        SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms: frame_wall_ms,
                frame_wall_ms,
            },
            frame_presented,
            gpu_order_preparation_pending: false,
            raster_execution_plan: SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            sort_refreshed: frame_presented && order_refreshed,
            order_uploaded: frame_presented
                && order_refreshed
                && order_backend == SurfaceOrderBackendUsed::Cpu,
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision,
            applied_order_revision: self.schedule.applied_order_revision(),
            presented_order_revision_lag: self
                .schedule
                .presented_order_revision_lag(camera_revision),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend,
            gpu_sort_fallback: false,
            adaptive_state: self.adaptive_state(),
            adaptive_gpu_failure: None,
            projected_draw_policy: self.projected_draw_policy(),
            projected_draw_execution,
            projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
            projected_draw_measurement_submission:
                SurfaceProjectedDrawMeasurementSubmission::NotRequested,
            completed_projected_draw_measurement: None,
            completed_projected_draw_measurement_failure: None,
            gpu_order_producer: match plan {
                PlanId::CpuPostSort => None,
                PlanId::GpuPostSort => Some(SurfaceGpuOrderProducer::PostSort),
                PlanId::GpuPreproject => Some(SurfaceGpuOrderProducer::Preproject),
            },
            gpu_producer_measurement_submission:
                SurfaceGpuProducerMeasurementSubmission::NotRequested,
            completed_gpu_producer_measurement: None,
            completed_gpu_producer_measurement_failure: None,
            order_measurement_submission,
            submitted_measurement_ticket: order_measurement_submission.ticket(),
            completed_order_measurement: None,
            completed_order_measurement_failure: None,
            visible_count_revision: submission
                .and_then(|submission| submission.visible_count())
                .map(|_| self.camera_revision),
            visible_count_pending: counts_pending,
            gpu_timestamp_queries_enabled: false,
        }
    }

    fn render_frame_sync(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let (completed_measurement, completed_measurement_failure) =
            self.collect_order_measurements();
        let (completed_projected_measurement, completed_projected_measurement_failure) =
            self.collect_projected_draw_measurements();
        let (completed_gpu_producer_measurement, completed_gpu_producer_measurement_failure) =
            self.collect_gpu_producer_measurements();
        self.refresh_adaptive_probe_owner();
        let has_order = match self.presented_order_backend {
            SurfaceOrderBackendUsed::Cpu => !self.renderer.current_sorted_indices().is_empty(),
            SurfaceOrderBackendUsed::Gpu => self.gpu_order_initialized,
        };
        let plan = self.schedule.plan(has_order);
        let mut adaptive_choice = self.pending_adaptive_choice.or_else(|| {
            if self.order_backend != SurfaceOrderBackend::Adaptive {
                return None;
            }
            if let Some(blocked) = self.blocked_order_choice {
                return Some(blocked);
            }
            plan.refresh_sort.then(|| {
                if matches!(
                    self.adaptive_probe_owner,
                    Some(AdaptiveProbeOwner::ProjectedCpu | AdaptiveProbeOwner::ProjectedGpu)
                ) {
                    self.adaptive_policy.held_refresh_choice()
                } else {
                    self.adaptive_policy.choose_refresh_backend()
                }
            })
        });
        let order_wants_formal_sample =
            adaptive_choice.is_some_and(|choice| choice.sample.is_some());
        if order_probe_owner_should_yield(
            self.adaptive_probe_owner,
            self.adaptive_policy.has_pending_sample(),
            plan.refresh_sort,
            order_wants_formal_sample,
        ) {
            // Order cohorts advance only on actual refreshes. Once their last
            // ticket is terminal, a stable frame would otherwise leave Order
            // owning an idle cohort and starve the projected learner. Yield
            // only the owner; phase/history resume on the next refresh.
            self.adaptive_probe_owner = None;
        }
        let planned_backend = match adaptive_choice {
            Some(choice) => choice.backend,
            None if !plan.refresh_sort => self.presented_order_backend,
            None => match self.order_backend {
                SurfaceOrderBackend::Cpu => SurfaceOrderBackendUsed::Cpu,
                SurfaceOrderBackend::Gpu => SurfaceOrderBackendUsed::Gpu,
                SurfaceOrderBackend::Adaptive => unreachable!("adaptive refresh has a choice"),
            },
        };
        let requested_backend = self.pending_order_backend.unwrap_or(planned_backend);
        let compact_available = self.presenter.projected_contributor_indirect_draw_enabled();
        let projected_choice_was_pending = self.pending_projected_choice.is_some();
        let mut projected_choice = self.pending_projected_choice.unwrap_or_else(|| {
            match self.projected_draw_policy {
                SurfaceProjectedDrawPolicy::Candidate => {
                    return ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Candidate,
                        sample: None,
                    };
                }
                SurfaceProjectedDrawPolicy::Compact => {
                    return ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Compact,
                        sample: None,
                    };
                }
                SurfaceProjectedDrawPolicy::Adaptive => {}
            }
            if self.presenter.raster_execution_plan()
                != SurfaceRasterExecutionPlan::ProjectedQuadsExact
            {
                return ProjectedAdaptiveChoice {
                    execution: SurfaceProjectedDrawExecution::Candidate,
                    sample: None,
                };
            }
            let owner = AdaptiveProbeOwner::projected(requested_backend);
            if !projected_policy_can_sample(
                self.adaptive_probe_owner,
                owner,
                self.adaptive_probe_owner == Some(AdaptiveProbeOwner::Order),
            ) {
                self.projected_policy(requested_backend).held_choice()
            } else {
                self.projected_policy_mut(requested_backend)
                    .choose(compact_available)
            }
        });
        let projected_owner = AdaptiveProbeOwner::projected(requested_backend);
        let projected_order_changed_this_frame = match requested_backend {
            SurfaceOrderBackendUsed::Cpu => {
                projected_order_changed(plan.refresh_sort, plan.upload_order, plan.refresh_sort)
            }
            SurfaceOrderBackendUsed::Gpu => {
                gpu_projected_order_changed(plan.refresh_sort, plan.refresh_sort)
            }
        };
        let projected_formal_sample_deferred =
            defer_projected_formal_choice(projected_choice, projected_order_changed_this_frame);
        let projected_claims_owner = projected_probe_claims_owner(
            projected_choice,
            projected_order_changed_this_frame,
            projected_choice_was_pending,
        );
        let projected_wants_transition_warmup = matches!(
            projected_choice.sample,
            Some(ProjectedAdaptiveSampleKind::TransitionWarmup)
        );
        if self.adaptive_probe_owner == Some(projected_owner)
            && projected_formal_sample_deferred
            && projected_choice_was_pending
        {
            // The first changed-order frame gives a new projected formal
            // choice one grace turn: the choice becomes pending so a following
            // cached-order frame can issue it. A second changed-order frame
            // must release ownership, otherwise continuous camera motion can
            // starve the order learner forever. Clear only the session ticket
            // request; the projected policy has not consumed its sample index
            // and will reissue it after Order finishes. Preserve execution so
            // a warmed challenger is not changed mid-frame.
            self.adaptive_probe_owner = None;
            self.pending_projected_choice = None;
            projected_choice.sample = None;
        }
        if matches!(
            self.adaptive_probe_owner,
            Some(AdaptiveProbeOwner::ProjectedCpu | AdaptiveProbeOwner::ProjectedGpu)
        ) && self.blocked_order_choice.is_some()
        {
            adaptive_choice = adaptive_choice.map(|choice| AdaptiveRefreshChoice {
                backend: choice.backend,
                sample: None,
            });
        } else if self.adaptive_probe_owner.is_none()
            && order_wants_formal_sample
            && projected_wants_transition_warmup
        {
            // A transition warmup may run while the order changes, but it is
            // not a formal projected ticket and must not retain arbitration
            // ownership. Delay the order sample for only this one frame so its
            // FrameCompletion evidence is not polluted by a raster-lane
            // transition.
            self.blocked_order_choice = adaptive_choice;
            adaptive_choice = adaptive_choice.map(|choice| AdaptiveRefreshChoice {
                backend: choice.backend,
                sample: None,
            });
        } else if self.adaptive_probe_owner.is_none()
            && order_wants_formal_sample
            && projected_claims_owner
        {
            // Stabilize the exact raster lane for the target order backend
            // before timing that order choice. The order policy has not
            // consumed evidence and will return this same formal choice after
            // the projected cohort reaches its owner boundary.
            self.blocked_order_choice = adaptive_choice;
            adaptive_choice = adaptive_choice.map(|choice| AdaptiveRefreshChoice {
                backend: choice.backend,
                sample: None,
            });
            self.adaptive_probe_owner =
                arbitrate_new_probe_owner(self.adaptive_probe_owner, true, true, projected_owner);
        } else if self.adaptive_probe_owner.is_none() && order_wants_formal_sample {
            self.blocked_order_choice = None;
            self.adaptive_probe_owner =
                arbitrate_new_probe_owner(self.adaptive_probe_owner, true, false, projected_owner);
        } else if self.adaptive_probe_owner.is_none() && projected_claims_owner {
            self.adaptive_probe_owner =
                arbitrate_new_probe_owner(self.adaptive_probe_owner, false, true, projected_owner);
        }
        // Every exact CPU refresh publishes the same frame-start -> queue-done
        // interval as GPU telemetry. Adaptive consumes only its matching
        // formal ticket, while forced-CPU benchmarks retain comparable
        // completion evidence instead of submit-wall timing.
        let track_cpu_completion =
            should_measure_cpu_refresh(plan, requested_backend, self.geometry_path());
        let mut gpu_failed = false;
        let mut output = if requested_backend == SurfaceOrderBackendUsed::Gpu
            && self.geometry_path() != GeometryPath::PagedActiveAtlas
        {
            match self.render_gpu_with_plan(plan, projected_choice) {
                Ok(output) => output,
                Err(error) if self.order_backend == SurfaceOrderBackend::Adaptive => {
                    let Some(reason) = adaptive_gpu_order_failure_reason(&error) else {
                        return Err(error);
                    };
                    gpu_failed = true;
                    self.adaptive_gpu_failure = Some(reason);
                    self.schedule.force_sort();
                    let fallback_plan = self
                        .schedule
                        .plan(!self.renderer.current_sorted_indices().is_empty());
                    projected_choice = self
                        .projected_policy(SurfaceOrderBackendUsed::Cpu)
                        .held_choice();
                    if self.adaptive_probe_owner == Some(AdaptiveProbeOwner::ProjectedGpu) {
                        self.adaptive_probe_owner = None;
                    }
                    let mut output = self.render_with_plan(
                        fallback_plan,
                        true,
                        fallback_plan.refresh_sort,
                        projected_choice,
                    )?;
                    output.gpu_sort_fallback = true;
                    output
                }
                Err(error) => return Err(error),
            }
        } else {
            self.render_with_plan(
                plan,
                plan.refresh_sort,
                track_cpu_completion,
                projected_choice,
            )?
        };
        // Keep one outer wall clock so a failed GPU attempt plus CPU fallback
        // is measured as the frame the caller actually experienced.
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        output.timings.frame_wall_ms = frame_wall_ms;
        output.stats.frame_ms = frame_wall_ms;
        if !output.frame_presented {
            // Exact WebGPU count/allocation is a preparation turn, not a
            // rendered frame. Preserve the frame-state plan, adaptive sample,
            // applied revision, and measurement ledger until the matching
            // scatter/raster/present submission exists.
            output.completed_order_measurement = completed_measurement;
            output.completed_order_measurement_failure = completed_measurement_failure;
            output.completed_projected_draw_measurement = completed_projected_measurement;
            output.completed_projected_draw_measurement_failure =
                completed_projected_measurement_failure;
            output.completed_gpu_producer_measurement = completed_gpu_producer_measurement;
            output.completed_gpu_producer_measurement_failure =
                completed_gpu_producer_measurement_failure;
            output.gpu_timestamp_queries_enabled = self.presenter.gpu_order_timestamps_enabled();
            self.pending_order_backend = Some(output.order_backend);
            self.pending_adaptive_choice = adaptive_choice;
            self.pending_projected_choice = Some(projected_choice);
            return Ok(output);
        }
        self.pending_order_backend = None;
        self.pending_adaptive_choice = None;
        let defer_projected_choice = defer_projected_formal_choice(
            projected_choice,
            output.sort_refreshed || output.order_uploaded,
        );
        // A formal Candidate/Compact ticket is comparable only when the
        // presented order stayed unchanged. Keep the exact requested choice
        // for the next stable-order frame instead of consuming its sample
        // index or silently turning it into an untimed frame.
        self.pending_projected_choice = defer_projected_choice.then_some(projected_choice);
        self.last_stats = output.stats;
        if self.order_backend == SurfaceOrderBackend::Adaptive {
            if gpu_failed {
                self.adaptive_policy.gpu_failed();
            } else if let Some(choice) = adaptive_choice {
                if choice.backend == SurfaceOrderBackendUsed::Gpu {
                    self.adaptive_gpu_failure = None;
                }
                match (choice.sample, self.adaptive_policy.metric()) {
                    (None, _) => {}
                    (Some(AdaptiveSampleKind::TransitionWarmup), AdaptiveMetric::OrderOnly) => {
                        // A successful submission is sufficient: queue order
                        // guarantees the next timed GPU command executes after
                        // this untimed transition. Waiting for readback here
                        // would multiply exploration cost without adding
                        // timing evidence.
                        self.adaptive_policy
                            .complete_synchronous_sample(choice, 0.0);
                    }
                    (Some(_), AdaptiveMetric::OrderOnly)
                        if choice.backend == SurfaceOrderBackendUsed::Cpu =>
                    {
                        self.adaptive_policy.complete_synchronous_sample(
                            choice,
                            output.stats.preprocess_ms + output.stats.sort_ms,
                        );
                    }
                    (Some(_), _) => {
                        if let Some(ticket) = output.order_measurement_submission.ticket() {
                            self.adaptive_policy.register_pending_sample(choice, ticket);
                        } else {
                            self.blocked_order_choice = Some(choice);
                            self.schedule.force_sort();
                        }
                    }
                }
            }
        }
        if !defer_projected_choice {
            match projected_choice.sample {
                None => {}
                Some(ProjectedAdaptiveSampleKind::TransitionWarmup) => self
                    .projected_policy_mut(output.order_backend)
                    .complete_synchronous_sample(projected_choice),
                Some(
                    ProjectedAdaptiveSampleKind::CandidateBootstrap
                    | ProjectedAdaptiveSampleKind::Probe(_),
                ) => {
                    if let Some(ticket) = output.projected_draw_measurement_submission.ticket() {
                        self.projected_policy_mut(output.order_backend)
                            .register_pending_sample(
                                output.order_backend,
                                projected_choice,
                                ticket,
                            );
                    }
                }
            }
        }
        output.completed_order_measurement = completed_measurement;
        output.completed_order_measurement_failure = completed_measurement_failure;
        output.completed_projected_draw_measurement = completed_projected_measurement;
        output.completed_projected_draw_measurement_failure =
            completed_projected_measurement_failure;
        output.completed_gpu_producer_measurement = completed_gpu_producer_measurement;
        output.completed_gpu_producer_measurement_failure =
            completed_gpu_producer_measurement_failure;
        output.gpu_timestamp_queries_enabled = self.presenter.gpu_order_timestamps_enabled();
        if output.order_backend == SurfaceOrderBackendUsed::Gpu {
            if let Some(measurement) = self.latest_gpu_order_measurement {
                output.stats.visible_count = measurement.visible_count;
                output.stats.drawn_count = measurement.drawn_count;
                output.visible_count_revision = Some(measurement.camera_revision);
                output.visible_count_pending = measurement.camera_revision != self.camera_revision;
            } else {
                output.stats.visible_count = 0;
                output.stats.drawn_count = 0;
                output.visible_count_revision = None;
                output.visible_count_pending = true;
            }
        } else if self.presenter.projected_contributor_indirect_draw_enabled() {
            if let Some(measurement) = self.latest_cpu_order_measurement {
                output.stats.visible_count = measurement.visible_count;
                output.stats.drawn_count = measurement.drawn_count;
                output.visible_count_revision = Some(measurement.camera_revision);
                output.visible_count_pending = measurement.camera_revision != self.camera_revision;
            } else {
                output.stats.visible_count = 0;
                output.stats.drawn_count = 0;
                output.visible_count_revision = None;
                output.visible_count_pending = true;
            }
        }
        if let Some(measurement) = completed_projected_measurement
            && measurement.order_backend == output.order_backend
        {
            output.stats.visible_count = measurement.visible_count;
            output.stats.drawn_count = measurement.drawn_count;
            output.visible_count_revision = Some(measurement.camera_revision);
            output.visible_count_pending = measurement.camera_revision != self.camera_revision;
        }
        output.adaptive_state = if self.order_backend == SurfaceOrderBackend::Adaptive {
            self.adaptive_policy.state()
        } else {
            SurfaceAdaptiveState::Disabled
        };
        output.adaptive_gpu_failure = self.adaptive_gpu_failure;
        output.projected_draw_adaptive_state =
            self.projected_draw_adaptive_state(output.order_backend);
        self.last_stats = output.stats;
        if output.sort_refreshed {
            self.schedule
                .record_applied_order(self.camera, self.camera_revision);
            output.applied_order_revision = self.schedule.applied_order_revision();
            output.presented_order_revision_lag = 0;
        }
        Ok(output)
    }

    fn collect_order_measurements(
        &mut self,
    ) -> (
        Option<SurfaceOrderMeasurement>,
        Option<SurfaceOrderMeasurementFailure>,
    ) {
        let cpu_telemetry = self.presenter.poll_cpu_order_completion_telemetry();
        for measurement in cpu_telemetry.completed {
            self.observe_cpu_completion_measurement(measurement);
            self.latest_cpu_order_measurement = Some(measurement);
            self.evidence.publish_cpu_order(measurement);
        }
        let mut newest_failure = None;
        for failure in cpu_telemetry.failures {
            if self.order_backend == SurfaceOrderBackend::Adaptive {
                self.adaptive_policy
                    .observe_cpu_measurement_failure(failure);
            }
            self.evidence.publish_order_failure(failure);
            newest_failure = Some(failure);
        }
        let telemetry = self.presenter.poll_gpu_order_telemetry();
        let mut newest = None;
        for measurement in telemetry.completed {
            if self.order_backend == SurfaceOrderBackend::Adaptive {
                self.adaptive_policy.observe_gpu_measurement(measurement);
            }
            self.latest_gpu_order_measurement = Some(measurement);
            self.evidence.publish_gpu_order(measurement);
            newest = Some(measurement);
        }
        for failure in telemetry.failures {
            if self.order_backend == SurfaceOrderBackend::Adaptive {
                self.adaptive_policy
                    .observe_gpu_measurement_failure(failure);
            }
            self.evidence.publish_order_failure(failure);
            newest_failure = Some(failure);
        }
        (newest, newest_failure)
    }

    fn collect_projected_draw_measurements(
        &mut self,
    ) -> (
        Option<SurfaceProjectedDrawMeasurement>,
        Option<SurfaceProjectedDrawMeasurementFailure>,
    ) {
        let telemetry = self.presenter.poll_projected_draw_telemetry();
        let mut newest = None;
        for measurement in telemetry.completed {
            if self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive
                && self.gpu_order_producer() == SurfaceGpuOrderProducer::PostSort
            {
                self.projected_policy_mut(measurement.order_backend)
                    .observe_measurement(measurement);
            }
            self.evidence.publish_projected_success(measurement);
            newest = Some(measurement);
        }
        let mut newest_failure = None;
        for failure in telemetry.failures {
            if self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive
                && self.gpu_order_producer() == SurfaceGpuOrderProducer::PostSort
            {
                self.projected_policy_mut(failure.order_backend)
                    .observe_failure(failure);
            }
            self.evidence.publish_projected_failure(failure);
            newest_failure = Some(failure);
        }
        self.refresh_adaptive_probe_owner();
        (newest, newest_failure)
    }

    fn collect_gpu_producer_measurements(
        &mut self,
    ) -> (
        Option<SurfaceGpuProducerMeasurement>,
        Option<SurfaceGpuProducerMeasurementFailure>,
    ) {
        let telemetry = self.presenter.poll_gpu_producer_telemetry();
        let mut newest = None;
        for measurement in telemetry.completed {
            self.evidence.publish_producer_success(measurement);
            newest = Some(measurement);
        }
        let mut newest_failure = None;
        for failure in telemetry.failures {
            self.evidence.publish_producer_failure(failure);
            newest_failure = Some(failure);
        }
        (newest, newest_failure)
    }

    fn observe_cpu_completion_measurement(&mut self, measurement: SurfaceCpuOrderMeasurement) {
        if self.order_backend == SurfaceOrderBackend::Adaptive
            && self.adaptive_policy.metric() == AdaptiveMetric::FrameCompletion
        {
            self.adaptive_policy.complete_pending_sample(
                SurfaceOrderBackendUsed::Cpu,
                measurement.ticket,
                measurement.frame_complete_ms,
            );
        }
    }

    fn render_gpu_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
        _projected_choice: ProjectedAdaptiveChoice,
    ) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let attempt = SessionFrameExecutor::attempt_direct_gpu(
            &mut self.presenter,
            &self.camera,
            plan.refresh_sort,
            self.camera_revision,
            frame_start,
        )?;
        match attempt.commit(|candidate| {
            let output = self.standalone_gpu_frame_output(candidate, plan, true);
            self.last_stats = output.stats;
            self.gpu_order_initialized |= plan.refresh_sort;
            self.presented_order_backend = SurfaceOrderBackendUsed::Gpu;
            self.schedule.finish_presented_frame(plan, false);
            output
        }) {
            Ok(output) => Ok(output),
            Err(candidate) => Ok(self.standalone_gpu_frame_output(candidate, plan, false)),
        }
    }

    fn standalone_gpu_frame_output(
        &self,
        candidate: StandaloneGpuFrameAttempt,
        plan: SurfaceFramePlan,
        frame_presented: bool,
    ) -> SurfaceFrameOutput {
        let presenter_submission = candidate.presenter_submission;
        let gpu_order_preparation_pending =
            presenter_submission == TelemetrySubmission::GpuOrderPreparationPending;
        let order_measurement_submission = SurfaceOrderMeasurementSubmission::from_presenter(
            SurfaceOrderBackendUsed::Gpu,
            presenter_submission,
        );
        let projected_draw_execution = candidate.projected_draw_execution;
        let projected_draw_measurement_submission =
            SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                projected_draw_execution,
                candidate.projected_draw_submission,
            );
        let gpu_order_producer = candidate.gpu_order_producer;
        let gpu_producer_measurement_submission =
            SurfaceGpuProducerMeasurementSubmission::from_presenter(
                gpu_order_producer.or_else(|| {
                    self.gpu_producer_measurement
                        .enabled()
                        .then_some(self.gpu_order_producer())
                }),
                candidate.gpu_producer_submission,
            );
        let submitted_measurement_ticket = order_measurement_submission.ticket();
        let stats = FrameStats {
            frame_ms: candidate.frame_wall_ms,
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms: 0.0,
            // The exact count arrives asynchronously from the same indirect
            // buffer used by this draw; zero here means pending, not sampled.
            visible_count: 0,
            drawn_count: 0,
        };
        SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms: candidate.render_submit_ms,
                frame_wall_ms: candidate.frame_wall_ms,
            },
            frame_presented,
            gpu_order_preparation_pending,
            raster_execution_plan: candidate.raster_execution_plan,
            sort_refreshed: frame_presented && plan.refresh_sort,
            order_uploaded: false,
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision: self.camera_revision,
            applied_order_revision: self.schedule.applied_order_revision(),
            presented_order_revision_lag: self
                .schedule
                .presented_order_revision_lag(self.camera_revision),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend: SurfaceOrderBackendUsed::Gpu,
            gpu_sort_fallback: false,
            adaptive_state: SurfaceAdaptiveState::Disabled,
            adaptive_gpu_failure: None,
            projected_draw_policy: self.projected_draw_policy,
            projected_draw_execution,
            projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
            projected_draw_measurement_submission,
            completed_projected_draw_measurement: None,
            completed_projected_draw_measurement_failure: None,
            gpu_order_producer,
            gpu_producer_measurement_submission,
            completed_gpu_producer_measurement: None,
            completed_gpu_producer_measurement_failure: None,
            order_measurement_submission,
            submitted_measurement_ticket,
            completed_order_measurement: None,
            completed_order_measurement_failure: None,
            visible_count_revision: None,
            visible_count_pending: submitted_measurement_ticket.is_some(),
            gpu_timestamp_queries_enabled: candidate.gpu_timestamp_queries_enabled,
        }
    }

    fn render_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
        sort_refreshed: bool,
        track_cpu_completion: bool,
        _projected_choice: ProjectedAdaptiveChoice,
    ) -> Result<SurfaceFrameOutput, RendererError> {
        let attempt = self.attempt_with_plan(plan, track_cpu_completion)?;
        match attempt.commit(|candidate| {
            let output = self.standalone_cpu_frame_output(candidate, plan, sort_refreshed, true);
            self.last_stats = output.stats;
            self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
            self.schedule
                .finish_presented_frame(plan, output.order_uploaded);
            output
        }) {
            Ok(output) => Ok(output),
            Err(candidate) => {
                Ok(self.standalone_cpu_frame_output(candidate, plan, sort_refreshed, false))
            }
        }
    }

    fn attempt_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
        track_cpu_completion: bool,
    ) -> Result<SessionFrameAttempt<StandaloneCpuFrameAttempt>, RendererError> {
        let frame_start = timer_now();
        SessionFrameExecutor::attempt_cpu_or_paged(
            &mut self.renderer,
            &mut self.presenter,
            &self.camera,
            self.camera_revision,
            frame_start,
            plan,
            track_cpu_completion,
        )
    }

    fn standalone_cpu_frame_output(
        &self,
        candidate: StandaloneCpuFrameAttempt,
        plan: SurfaceFramePlan,
        sort_refreshed: bool,
        frame_presented: bool,
    ) -> SurfaceFrameOutput {
        let presenter_submission = candidate.presenter_submission;
        let gpu_order_preparation_pending =
            presenter_submission == TelemetrySubmission::GpuOrderPreparationPending;
        let order_measurement_submission = SurfaceOrderMeasurementSubmission::from_presenter(
            SurfaceOrderBackendUsed::Cpu,
            presenter_submission,
        );
        let projected_draw_execution = candidate.projected_draw_execution;
        let projected_draw_measurement_submission =
            SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                projected_draw_execution,
                candidate.projected_draw_submission,
            );
        let submitted_measurement_ticket = order_measurement_submission.ticket();
        SurfaceFrameOutput {
            stats: candidate.stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms: candidate.render_submit_ms,
                frame_wall_ms: candidate.frame_wall_ms,
            },
            frame_presented,
            gpu_order_preparation_pending,
            raster_execution_plan: candidate.raster_execution_plan,
            sort_refreshed: frame_presented && (candidate.paged || sort_refreshed),
            order_uploaded: frame_presented && (candidate.paged || plan.upload_order),
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision: self.camera_revision,
            applied_order_revision: self.schedule.applied_order_revision(),
            presented_order_revision_lag: self
                .schedule
                .presented_order_revision_lag(self.camera_revision),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_sort_fallback: false,
            adaptive_state: SurfaceAdaptiveState::Disabled,
            adaptive_gpu_failure: None,
            projected_draw_policy: self.projected_draw_policy,
            projected_draw_execution,
            projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
            projected_draw_measurement_submission,
            completed_projected_draw_measurement: None,
            completed_projected_draw_measurement_failure: None,
            gpu_order_producer: None,
            gpu_producer_measurement_submission:
                SurfaceGpuProducerMeasurementSubmission::NotRequested,
            completed_gpu_producer_measurement: None,
            completed_gpu_producer_measurement_failure: None,
            order_measurement_submission,
            submitted_measurement_ticket,
            completed_order_measurement: None,
            completed_order_measurement_failure: None,
            visible_count_revision: Some(if candidate.paged || plan.refresh_sort {
                self.camera_revision
            } else {
                self.schedule.applied_order_revision()
            }),
            visible_count_pending: !candidate.paged
                && !plan.refresh_sort
                && self.schedule.applied_order_revision() != self.camera_revision,
            gpu_timestamp_queries_enabled: candidate.gpu_timestamp_queries_enabled,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_frame_async_sort(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        if self.pending_async_completion.is_none() {
            let mut async_poll = self
                .schedule
                .poll_async_order(&self.camera, self.camera_revision)?;
            let accepted_order = if let Some(mut candidate) = async_poll.candidate.take() {
                let accepted_order = PendingAsyncOrder {
                    camera: candidate.camera,
                    camera_revision: candidate.camera_revision,
                    revision_lag: candidate.revision_lag,
                };
                // The renderer cache is attempt-prepared input. Its public
                // Session identity remains pending until this order presents.
                let replace_result = self
                    .renderer
                    .replace_surface_sorted_indices_recycling(&mut candidate.ordered_ids);
                self.schedule.recycle_async_order(candidate.ordered_ids);
                replace_result?;
                Some(accepted_order)
            } else {
                None
            };
            if async_poll.completed_revision.is_some() {
                self.pending_async_completion = Some(PendingAsyncCompletion {
                    accepted_order,
                    completed_timing: async_poll.completed_timing,
                    observed_revision_lag: async_poll.observed_revision_lag,
                    stale_result_dropped: async_poll.stale_result_dropped,
                    completed_revision: async_poll.completed_revision,
                });
            }
        }

        if let Some(completion) = self.pending_async_completion.as_mut()
            && let Some(order) = completion.accepted_order
        {
            if let Some(revision_lag) = self.schedule.pending_async_order_revision_lag(
                &order.camera,
                order.camera_revision,
                &self.camera,
                self.camera_revision,
            ) {
                completion
                    .accepted_order
                    .as_mut()
                    .expect("copied pending async order")
                    .revision_lag = revision_lag;
                completion.observed_revision_lag = Some(revision_lag);
            } else {
                completion.accepted_order = None;
                completion.stale_result_dropped = true;
                completion.observed_revision_lag = Some(
                    u32::try_from(self.camera_revision.saturating_sub(order.camera_revision))
                        .unwrap_or(u32::MAX),
                );
            }
        }

        let has_order = !self.renderer.current_sorted_indices().is_empty();
        if self.schedule.requires_initial_sync(has_order) {
            if let Some(completion) = self.pending_async_completion.as_mut()
                && completion.accepted_order.take().is_some()
            {
                completion.stale_result_dropped = true;
            }
            let mut output = self.render_frame_sync()?;
            if output.frame_presented {
                self.commit_presented_async_completion(&mut output);
            }
            return Ok(output);
        }

        let pending_order = self
            .pending_async_completion
            .and_then(|completion| completion.accepted_order);
        if pending_order.is_none()
            && self
                .schedule
                .requires_stale_order_fallback(&self.camera, self.camera_revision)
        {
            let mut output = self.render_frame_sync()?;
            output.sync_sort_fallback = true;
            if output.frame_presented {
                self.commit_presented_async_completion(&mut output);
            }
            return Ok(output);
        }

        let mut projected_policy = self.adaptive_projected_cpu.clone();
        let mut projected_probe_owner = self.adaptive_probe_owner;
        let compact_available = self.presenter.projected_contributor_indirect_draw_enabled();
        let projected_choice =
            self.pending_projected_choice
                .unwrap_or_else(|| match self.projected_draw_policy {
                    SurfaceProjectedDrawPolicy::Candidate => ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Candidate,
                        sample: None,
                    },
                    SurfaceProjectedDrawPolicy::Compact => ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Compact,
                        sample: None,
                    },
                    SurfaceProjectedDrawPolicy::Adaptive => {
                        projected_policy.choose(compact_available)
                    }
                });
        if projected_probe_owner.is_none() && projected_choice.sample.is_some() {
            projected_probe_owner = Some(AdaptiveProbeOwner::ProjectedCpu);
        }
        let mut plan = self.schedule.stable_order_plan();
        plan.upload_order |= pending_order.is_some();
        let attempt = self.attempt_with_plan(plan, false)?;
        let mut output = match attempt {
            SessionFrameAttempt::Presented(candidate) => {
                let output = self.standalone_cpu_frame_output(
                    candidate,
                    plan,
                    pending_order.is_some(),
                    true,
                );
                self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
                output
            }
            SessionFrameAttempt::Unavailable(candidate) => {
                let mut output = self.standalone_cpu_frame_output(candidate, plan, false, false);
                output.projected_draw_adaptive_state =
                    self.projected_draw_adaptive_state(SurfaceOrderBackendUsed::Cpu);
                return Ok(output);
            }
        };

        self.adaptive_projected_cpu = projected_policy;
        self.adaptive_probe_owner = projected_probe_owner;

        let (completed_projected_measurement, completed_projected_measurement_failure) =
            self.collect_projected_draw_measurements();
        let (completed_gpu_producer_measurement, completed_gpu_producer_measurement_failure) =
            self.collect_gpu_producer_measurements();
        output.completed_projected_draw_measurement = completed_projected_measurement;
        output.completed_projected_draw_measurement_failure =
            completed_projected_measurement_failure;
        output.completed_gpu_producer_measurement = completed_gpu_producer_measurement;
        output.completed_gpu_producer_measurement_failure =
            completed_gpu_producer_measurement_failure;

        {
            let defer_projected_choice = defer_projected_formal_choice(
                projected_choice,
                output.sort_refreshed || output.order_uploaded,
            );
            self.pending_projected_choice = defer_projected_choice.then_some(projected_choice);
            if !defer_projected_choice {
                match projected_choice.sample {
                    None => {}
                    Some(ProjectedAdaptiveSampleKind::TransitionWarmup) => self
                        .adaptive_projected_cpu
                        .complete_synchronous_sample(projected_choice),
                    Some(
                        ProjectedAdaptiveSampleKind::CandidateBootstrap
                        | ProjectedAdaptiveSampleKind::Probe(_),
                    ) => {
                        if let Some(ticket) = output.projected_draw_measurement_submission.ticket()
                        {
                            self.adaptive_projected_cpu.register_pending_sample(
                                SurfaceOrderBackendUsed::Cpu,
                                projected_choice,
                                ticket,
                            );
                        }
                    }
                }
            }
        }
        output.projected_draw_adaptive_state =
            self.projected_draw_adaptive_state(SurfaceOrderBackendUsed::Cpu);
        self.commit_presented_async_completion(&mut output);
        Ok(output)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn commit_presented_async_completion(&mut self, output: &mut SurfaceFrameOutput) {
        debug_assert!(output.frame_presented);
        let completion = self.pending_async_completion.take();
        let accepted_order = completion.and_then(|completion| completion.accepted_order);
        if let Some(order) = accepted_order {
            self.schedule.accept_async_order(
                order.camera,
                order.camera_revision,
                order.revision_lag,
            );
        }
        self.schedule
            .finish_presented_frame(self.schedule.stable_order_plan(), output.order_uploaded);

        if let Some(completion) = completion {
            if let Some((preprocess_ms, sort_ms)) = completion.completed_timing {
                output.stats.preprocess_ms = preprocess_ms;
                output.stats.sort_ms = sort_ms;
            }
            output.async_sort_revision_lag = completion.observed_revision_lag;
            output.stale_async_sort_dropped = completion.stale_result_dropped;
            output.async_sort_completed_revision = completion.completed_revision;
            output.async_sort_result_applied = completion.accepted_order.is_some();
        }
        if let Some(order) = accepted_order {
            output.visible_count_revision = Some(order.camera_revision);
            output.visible_count_pending = order.camera_revision != self.camera_revision;
        }

        let should_schedule = self.schedule.should_schedule_async();
        if should_schedule {
            self.schedule
                .start_async_order(self.camera, self.camera_revision);
        }
        output.async_sort_scheduled = should_schedule;
        output.camera_revision = self.camera_revision;
        output.applied_order_revision = self.schedule.applied_order_revision();
        output.presented_order_revision_lag = self
            .schedule
            .presented_order_revision_lag(self.camera_revision);
        output.async_sort_scheduled_revision = should_schedule.then_some(self.camera_revision);
        self.last_stats = output.stats;
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "surface_session_exact_control_tests.rs"]
mod exact_control_tests;

#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_INITIAL_PROBE_DELAY, AdaptiveMetric, AdaptiveOrderPolicy, AdaptiveProbeOwner,
        AdaptiveProjectedDrawPolicy, AdaptiveSampleKind, PlanId, ProjectedAdaptiveChoice,
        ProjectedAdaptiveSampleKind, SurfaceAdaptiveState, SurfaceGeometrySwitchEntry,
        SurfaceGpuProducerMeasurementControl, SurfaceGpuProducerMeasurementSubmission,
        SurfaceGpuProducerMeasurementUnsampledReason, SurfaceOrderBackend, SurfaceOrderBackendUsed,
        SurfaceOrderMeasurementSubmission, SurfaceOrderMeasurementUnsampledReason,
        SurfaceProjectedDrawAdaptiveState, SurfaceProjectedDrawMeasurementSubmission,
        SurfaceProjectedDrawMeasurementUnsampledReason, SurfaceProjectedDrawPolicy,
        SurfaceSortSchedule, TelemetrySubmission, adaptive_gpu_order_failure_reason,
        adaptive_primary_metric, arbitrate_new_probe_owner, defer_projected_formal_choice,
        exact_order_refreshed, exact_published_camera_revision,
        gpu_producer_measurement_context_is_valid, gpu_projected_order_changed,
        legacy_surface_current_stats_poll, legacy_surface_current_stats_request,
        legacy_surface_current_stats_submission, order_probe_owner_should_yield,
        paged_surface_counts, projected_formal_sample_requested, projected_order_changed,
        projected_policy_can_sample, projected_probe_claims_owner,
        reset_adaptive_for_gpu_producer_measurement_transition,
        reset_adaptive_for_raster_transition, should_measure_cpu_refresh,
        should_reset_order_for_projected_incumbent_change, surface_geometry_switch_entry,
        try_switch_renderer_geometry_path, validate_gpu_order_producer_transition,
        validate_projected_draw_policy_transition,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use super::{
        ExactSurfacePlanState, SurfaceRenderSession, async_sort_supported, publish_only_on_present,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use crate::surface::SessionSurfaceOwner;
    use crate::{
        GeometryPath, Renderer, RendererError, ResidentGpuError, ResidentSceneCpu,
        SurfaceCompatibilityChannel, SurfaceCurrentStatsPoll, SurfaceCurrentStatsRequest,
        SurfaceCurrentStatsSubmission, SurfaceCurrentStatsUnsampledReason, SurfaceGpuOrderProducer,
        SurfacePresenterError, SurfaceProjectedDrawExecution, SurfaceRasterExecutionPlan,
    };

    #[test]
    fn exact_surface_setters_share_the_same_transaction() {
        let source = include_str!("surface_session.rs");
        assert_eq!(
            source
                .matches(concat!("try_set_exact_gpu_order_", "producer(producer)?"))
                .count(),
            2,
            "sync and Web async setters must share the Exact transaction"
        );
    }
    use gsplat_core::{Camera, RendererConfig, SceneBuffers, Vec3f};
    #[cfg(not(target_arch = "wasm32"))]
    const EXACT_STATES: [ExactSurfacePlanState; 4] = [
        ExactSurfacePlanState::CpuPostSort,
        ExactSurfacePlanState::GpuPostSort,
        ExactSurfacePlanState::GpuPreproject,
        ExactSurfacePlanState::Adaptive,
    ];

    #[cfg(not(target_arch = "wasm32"))]
    fn canonical_exact_tuple(
        state: ExactSurfacePlanState,
    ) -> (
        SurfaceOrderBackend,
        SurfaceProjectedDrawPolicy,
        SurfaceGpuOrderProducer,
        SurfaceRasterExecutionPlan,
    ) {
        (
            state.order_backend(),
            state.projected_policy(),
            state.producer(),
            SurfaceRasterExecutionPlan::ProjectedQuadsExact,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn every_exact_compatibility_setter_has_a_closed_four_state_transition_table() {
        use ExactSurfacePlanState::{Adaptive, CpuPostSort, GpuPostSort, GpuPreproject};

        let order_cases = [
            (SurfaceOrderBackend::Cpu, [CpuPostSort; 4]),
            (
                SurfaceOrderBackend::Gpu,
                [GpuPostSort, GpuPostSort, GpuPreproject, GpuPostSort],
            ),
            (SurfaceOrderBackend::Adaptive, [Adaptive; 4]),
        ];
        for (backend, expected) in order_cases {
            for (index, current) in EXACT_STATES.into_iter().enumerate() {
                let next = current.with_order_backend(backend);
                assert_eq!(next, expected[index]);
                let _closed_tuple = canonical_exact_tuple(next);
            }
        }

        let projected_cases = [
            (
                SurfaceProjectedDrawPolicy::Candidate,
                [
                    Some(CpuPostSort),
                    Some(GpuPostSort),
                    Some(GpuPostSort),
                    None,
                ],
            ),
            (
                SurfaceProjectedDrawPolicy::Compact,
                [None, Some(GpuPreproject), Some(GpuPreproject), None],
            ),
            (
                SurfaceProjectedDrawPolicy::Adaptive,
                [
                    Some(CpuPostSort),
                    Some(GpuPostSort),
                    Some(GpuPostSort),
                    Some(Adaptive),
                ],
            ),
        ];
        for (policy, expected) in projected_cases {
            for (index, current) in EXACT_STATES.into_iter().enumerate() {
                match (current.with_projected_policy(policy), expected[index]) {
                    (Ok(next), Some(expected)) => {
                        assert_eq!(next, expected);
                        let _closed_tuple = canonical_exact_tuple(next);
                    }
                    (Err(_), None) => {}
                    (actual, expected) => panic!(
                        "projected transition mismatch: current={current:?} policy={policy:?} actual_ok={} expected={expected:?}",
                        actual.is_ok()
                    ),
                }
            }
        }

        let producer_cases = [
            (
                SurfaceGpuOrderProducer::PostSort,
                [
                    Some(CpuPostSort),
                    Some(GpuPostSort),
                    Some(GpuPostSort),
                    Some(Adaptive),
                ],
            ),
            (
                SurfaceGpuOrderProducer::Preproject,
                [None, Some(GpuPreproject), Some(GpuPreproject), None],
            ),
        ];
        for (producer, expected) in producer_cases {
            for (index, current) in EXACT_STATES.into_iter().enumerate() {
                match (current.with_producer(producer), expected[index]) {
                    (Ok(next), Some(expected)) => {
                        assert_eq!(next, expected);
                        let _closed_tuple = canonical_exact_tuple(next);
                    }
                    (Err(_), None) => {}
                    (actual, expected) => panic!(
                        "producer transition mismatch: current={current:?} producer={producer:?} actual_ok={} expected={expected:?}",
                        actual.is_ok()
                    ),
                }
            }
        }

        for current in EXACT_STATES {
            assert!(matches!(
                current.with_raster(SurfaceRasterExecutionPlan::ProjectedQuadsExact),
                Ok(next) if next == current
            ));
            assert!(
                current
                    .with_raster(SurfaceRasterExecutionPlan::GlobalQuads)
                    .is_err()
            );
            assert!(matches!(
                current.with_geometry(GeometryPath::PackedAtlas),
                Ok(next) if next == current
            ));
            assert!(
                current
                    .with_geometry(GeometryPath::SortedIndexDirect)
                    .is_err()
            );
            assert!(
                current
                    .with_geometry(GeometryPath::PagedActiveAtlas)
                    .is_err()
            );
            assert!(matches!(
                current.with_gpu_producer_measurement(false),
                Ok(next) if next == current
            ));
            assert!(current.with_gpu_producer_measurement(true).is_err());
            assert!(matches!(current.with_sort_interval(1), Ok(next) if next == current));
            assert!(current.with_sort_interval(2).is_err());
            assert!(matches!(
                current.with_sort_schedule(SurfaceSortSchedule::Interval(1)),
                Ok(next) if next == current
            ));
            assert!(
                current
                    .with_sort_schedule(SurfaceSortSchedule::Interval(2))
                    .is_err()
            );
            assert!(
                current
                    .with_sort_schedule(SurfaceSortSchedule::AsyncLatest { interval: 1 })
                    .is_err()
            );
            assert!(matches!(current.with_async_sort(false), Ok(next) if next == current));
            assert!(current.with_async_sort(true).is_err());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ffi_swift_and_kotlin_default_apply_sequences_end_in_canonical_tuples() {
        use ExactSurfacePlanState::{Adaptive, CpuPostSort, GpuPostSort, GpuPreproject};

        // FFI constructor: session default CPU, then non-Paged order Adaptive.
        let ffi = CpuPostSort.with_order_backend(SurfaceOrderBackend::Adaptive);
        assert_eq!(ffi, Adaptive);

        // Swift apply(options): interval=1, async=false, order, frame latency
        // (lifecycle-only), then projected. Defaults are both Adaptive;
        // forced order options followed by projected Adaptive must remain
        // valid canonical Candidate tuples.
        let swift_base = ffi
            .with_sort_interval(1)
            .expect("Swift default exact schedule")
            .with_async_sort(false)
            .expect("Swift default synchronous order");
        let swift_default = swift_base
            .with_order_backend(SurfaceOrderBackend::Adaptive)
            .with_projected_policy(SurfaceProjectedDrawPolicy::Adaptive)
            .expect("Swift default projected apply");
        let swift_cpu = swift_base
            .with_order_backend(SurfaceOrderBackend::Cpu)
            .with_projected_policy(SurfaceProjectedDrawPolicy::Adaptive)
            .expect("Swift forced CPU then projected Adaptive");
        let swift_gpu = swift_base
            .with_order_backend(SurfaceOrderBackend::Gpu)
            .with_projected_policy(SurfaceProjectedDrawPolicy::Adaptive)
            .expect("Swift forced GPU then projected Adaptive");
        assert_eq!(swift_default, Adaptive);
        assert_eq!(swift_cpu, CpuPostSort);
        assert_eq!(swift_gpu, GpuPostSort);

        // Kotlin configure: measurement=false, producer=PostSort, interval=1,
        // async=false, order, frame latency (lifecycle-only), then projected.
        // Its optional diagnostics path applies Compact before switching to
        // Preproject; M2b's measurement collector remains rejected separately.
        let kotlin_default = ffi
            .with_gpu_producer_measurement(false)
            .expect("default observer disable")
            .with_producer(SurfaceGpuOrderProducer::PostSort)
            .expect("Kotlin default PostSort")
            .with_sort_interval(1)
            .expect("Kotlin default exact schedule")
            .with_async_sort(false)
            .expect("Kotlin default synchronous order")
            .with_order_backend(SurfaceOrderBackend::Adaptive)
            .with_projected_policy(SurfaceProjectedDrawPolicy::Adaptive)
            .expect("Kotlin default projected apply");
        let kotlin_preproject = ffi
            .with_gpu_producer_measurement(false)
            .expect("Kotlin diagnostic observer reset")
            .with_producer(SurfaceGpuOrderProducer::PostSort)
            .expect("Kotlin initial PostSort")
            .with_sort_interval(1)
            .expect("Kotlin diagnostic exact schedule")
            .with_async_sort(false)
            .expect("Kotlin diagnostic synchronous order")
            .with_order_backend(SurfaceOrderBackend::Gpu)
            .with_projected_policy(SurfaceProjectedDrawPolicy::Compact)
            .expect("Kotlin forced Compact")
            .with_producer(SurfaceGpuOrderProducer::Preproject)
            .expect("Kotlin Preproject");
        assert!(
            kotlin_preproject
                .with_gpu_producer_measurement(true)
                .is_err(),
            "M2a rejects the M2b collector before mutating the complete plan"
        );
        assert_eq!(kotlin_default, Adaptive);
        assert_eq!(kotlin_preproject, GpuPreproject);

        for state in [
            ffi,
            swift_default,
            swift_cpu,
            swift_gpu,
            kotlin_default,
            kotlin_preproject,
        ] {
            let _closed_tuple = canonical_exact_tuple(state);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn current_stats_snapshot_commits_only_a_successfully_presented_candidate() {
        let mut published = 41_u64;
        publish_only_on_present(&mut published, None);
        assert_eq!(published, 41, "acquire-none preserves the last snapshot");
        publish_only_on_present(&mut published, None);
        assert_eq!(published, 41, "present failure preserves the last snapshot");
        publish_only_on_present(&mut published, Some(42));
        assert_eq!(
            published, 42,
            "successful present atomically commits its snapshot"
        );
    }

    #[test]
    fn trace_camera_frames_publish_renderer_owned_current_stats_revisions() {
        assert_eq!(
            exact_published_camera_revision(2, Some(1)),
            1,
            "the first trace frame may collapse multiple pre-present camera mutations"
        );
        assert_eq!(
            exact_published_camera_revision(3, Some(2)),
            2,
            "the next trace frame advances with the renderer-owned identity"
        );
        assert_eq!(
            exact_published_camera_revision(3, None),
            3,
            "an acquire-without-present has no renderer frame identity to publish"
        );
    }

    #[test]
    fn moving_trace_plan_transition_cannot_alias_a_fresh_order_generation() {
        assert!(exact_order_refreshed(None, PlanId::CpuPostSort, 1));
        assert!(!exact_order_refreshed(
            Some((PlanId::CpuPostSort, 1)),
            PlanId::CpuPostSort,
            1,
        ));
        assert!(
            exact_order_refreshed(Some((PlanId::GpuPostSort, 17)), PlanId::CpuPostSort, 17,),
            "independent Adaptive plan generations cannot alias the moving trace refresh"
        );
        assert!(
            exact_order_refreshed(Some((PlanId::CpuPostSort, 23)), PlanId::GpuPreproject, 23,),
            "the Preproject generation domain is independent too"
        );
    }

    #[test]
    fn sort_schedule_exposes_interval_for_sync_and_async_policies() {
        assert_eq!(SurfaceSortSchedule::Interval(2).interval(), 2);
        assert_eq!(
            SurfaceSortSchedule::AsyncLatest { interval: 3 }.interval(),
            3
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn async_sort_support_is_limited_to_direct_cpu_execution() {
        for path in [
            GeometryPath::SortedIndexDirect,
            GeometryPath::PackedAtlas,
            GeometryPath::PagedActiveAtlas,
        ] {
            for backend in [
                SurfaceOrderBackend::Cpu,
                SurfaceOrderBackend::Gpu,
                SurfaceOrderBackend::Adaptive,
            ] {
                assert_eq!(
                    async_sort_supported(path, backend),
                    path == GeometryPath::SortedIndexDirect && backend == SurfaceOrderBackend::Cpu,
                    "unexpected async-sort support for path={path:?} backend={backend:?}",
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn public_async_session_publishes_completion_only_after_presented_retry() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0), Vec3f::new(0.1, 0.0, 1.2)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[-3.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut renderer =
            Renderer::with_config_for_surface(RendererConfig::default()).expect("test renderer");
        renderer.load_scene(scene).expect("test Direct scene");
        let presenter = SessionSurfaceOwner::test_direct(2, [true]);
        let mut session =
            SurfaceRenderSession::new_with_surface_owner(renderer, presenter, Camera::default())
                .expect("public Surface session");
        session
            .set_async_sort_enabled(true)
            .expect("Direct CPU async scheduling");

        let initial = session.render_frame().expect("initial presented frame");
        assert!(initial.frame_presented);
        let published_stats = session.last_stats();
        let published_order_submission = session
            .compatibility_submission(SurfaceCompatibilityChannel::Order)
            .expect("initial compatibility order submission");
        let published_projected_submission = session
            .compatibility_submission(SurfaceCompatibilityChannel::Projected)
            .expect("initial compatibility projected submission");
        let published_current_stats = session.current_stats_submission();
        let published_projected_state =
            session.projected_draw_adaptive_state(SurfaceOrderBackendUsed::Cpu);
        assert_eq!(initial.applied_order_revision, 0);

        let mut moved_camera = session.camera();
        moved_camera.pose.position.x += 0.0001;
        session
            .set_camera(moved_camera)
            .expect("compatible camera revision");
        assert_eq!(session.camera_revision(), 1);

        session.schedule.start_async_order(moved_camera, 1);
        let unavailable = (0..1_000)
            .find_map(|_| {
                session.presenter.push_test_frame_presented(false);
                let output = session
                    .render_frame()
                    .expect("unavailable drawable is not an error");
                assert!(!output.frame_presented);
                if session.pending_async_completion.is_some() {
                    Some(output)
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    None
                }
            })
            .expect("native async completion became attempt input");
        let completed_timing = session
            .pending_async_completion
            .expect("completion remains pending until present")
            .completed_timing
            .expect("native async timing");
        assert!(!unavailable.frame_presented);
        assert_eq!(unavailable.applied_order_revision, 0);
        assert_eq!(unavailable.async_sort_completed_revision, None);
        assert!(!unavailable.async_sort_result_applied);
        assert!(!unavailable.async_sort_scheduled);
        assert_eq!(session.last_stats(), published_stats);
        assert_eq!(session.current_stats_submission(), published_current_stats);
        assert_eq!(
            session.projected_draw_adaptive_state(SurfaceOrderBackendUsed::Cpu),
            published_projected_state,
        );
        assert_eq!(
            session.compatibility_submission(SurfaceCompatibilityChannel::Order),
            Some(published_order_submission),
        );
        assert_eq!(
            session.compatibility_submission(SurfaceCompatibilityChannel::Projected),
            Some(published_projected_submission),
        );
        assert_eq!(session.schedule.applied_order_revision(), 0);
        assert!(session.pending_async_completion.is_some());

        session.presenter.push_test_frame_presented(true);
        let presented = session.render_frame().expect("presented retry");
        assert!(presented.frame_presented);
        assert!(presented.sort_refreshed);
        assert!(presented.order_uploaded);
        assert_eq!(presented.applied_order_revision, 1);
        assert_eq!(presented.presented_order_revision_lag, 0);
        assert_eq!(presented.async_sort_completed_revision, Some(1));
        assert_eq!(presented.async_sort_revision_lag, Some(0));
        assert!(presented.async_sort_result_applied);
        assert_eq!(presented.visible_count_revision, Some(1));
        assert!(!presented.visible_count_pending);
        assert_eq!(
            (presented.stats.preprocess_ms, presented.stats.sort_ms),
            completed_timing,
        );
        assert_eq!(session.last_stats(), presented.stats);
        assert_eq!(session.schedule.applied_order_revision(), 1);
        assert!(session.pending_async_completion.is_none());
        assert_eq!(session.current_stats_submission(), published_current_stats);
        assert_ne!(
            session.compatibility_submission(SurfaceCompatibilityChannel::Order),
            Some(published_order_submission),
        );
    }

    #[test]
    fn web_geometry_switch_matrix_is_constructor_only() {
        let paths = [
            GeometryPath::SortedIndexDirect,
            GeometryPath::PackedAtlas,
            GeometryPath::PagedActiveAtlas,
        ];
        for current in paths {
            for target in paths {
                let expected = if current == target {
                    SurfaceGeometrySwitchEntry::AlreadyActive
                } else {
                    SurfaceGeometrySwitchEntry::Unsupported
                };
                assert_eq!(
                    surface_geometry_switch_entry(true, current, target),
                    expected,
                    "Web geometry transition {current:?} -> {target:?}",
                );
            }
        }
    }

    #[test]
    fn native_geometry_switch_matrix_preserves_direct_paged_transactions() {
        assert_eq!(
            surface_geometry_switch_entry(
                false,
                GeometryPath::SortedIndexDirect,
                GeometryPath::PagedActiveAtlas,
            ),
            SurfaceGeometrySwitchEntry::Synchronous,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                false,
                GeometryPath::PagedActiveAtlas,
                GeometryPath::SortedIndexDirect,
            ),
            SurfaceGeometrySwitchEntry::Synchronous,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                false,
                GeometryPath::SortedIndexDirect,
                GeometryPath::PackedAtlas,
            ),
            SurfaceGeometrySwitchEntry::Unsupported,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                false,
                GeometryPath::PackedAtlas,
                GeometryPath::PagedActiveAtlas,
            ),
            SurfaceGeometrySwitchEntry::Unsupported,
        );
    }

    #[test]
    fn legacy_packed_surface_stats_never_target_the_offscreen_exact_runtime() {
        let mut renderer = match Renderer::with_config(RendererConfig {
            width: 64,
            height: 64,
            ..RendererConfig::default()
        }) {
            Ok(renderer) => renderer,
            #[cfg(target_os = "macos")]
            Err(error) => panic!("required legacy Surface Metal renderer unavailable: {error}"),
            #[cfg(not(target_os = "macos"))]
            Err(error) => {
                eprintln!("skipping optional legacy Surface GPU test: {error}");
                return;
            }
        };
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        let resident = ResidentSceneCpu::encode_owned(SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[-1.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.1, 0.2, 0.3]],
            sh_degree: 0,
            sh_rest: None,
        })
        .expect("legacy Packed Surface fixture");
        renderer
            .load_resident_scene(resident)
            .expect("offscreen Exact runtime proves the regression precondition");
        let runtime = renderer
            .exact_offscreen_runtime
            .as_ref()
            .expect("Packed renderer owns the unrelated M1 offscreen runtime");
        assert!(!runtime.current_stats_request_pending_for_test());

        for _ in 0..2 {
            assert_eq!(
                legacy_surface_current_stats_request(&mut renderer),
                SurfaceCurrentStatsRequest::Unsampled(
                    SurfaceCurrentStatsUnsampledReason::GpuUnavailable
                )
            );
            assert_eq!(
                legacy_surface_current_stats_submission(),
                SurfaceCurrentStatsSubmission::NotRequested
            );
            assert_eq!(
                legacy_surface_current_stats_poll(&mut renderer),
                SurfaceCurrentStatsPoll::Empty
            );
            assert!(
                !renderer
                    .exact_offscreen_runtime
                    .as_ref()
                    .expect("offscreen runtime remains installed")
                    .current_stats_request_pending_for_test()
            );
        }
    }

    #[test]
    fn every_non_paged_cpu_refresh_uses_completion_telemetry() {
        let refresh = super::SurfaceFramePlan {
            refresh_sort: true,
            upload_order: true,
        };
        let reuse = super::SurfaceFramePlan {
            refresh_sort: false,
            upload_order: false,
        };
        assert!(should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::SortedIndexDirect,
        ));
        assert!(should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::PackedAtlas,
        ));
        assert!(!should_measure_cpu_refresh(
            reuse,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::PackedAtlas,
        ));
        assert!(!should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Gpu,
            GeometryPath::PackedAtlas,
        ));
        assert!(!should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::PagedActiveAtlas,
        ));
    }

    #[test]
    fn gpu_order_preparation_exposes_no_formal_measurement_identity() {
        let submission = SurfaceOrderMeasurementSubmission::from_presenter(
            SurfaceOrderBackendUsed::Gpu,
            TelemetrySubmission::GpuOrderPreparationPending,
        );
        assert_eq!(submission, SurfaceOrderMeasurementSubmission::NotRequested);
        assert_eq!(submission.ticket(), None);
    }

    #[test]
    fn submission_conversions_preserve_issued_and_unsampled_identity() {
        assert_eq!(
            SurfaceOrderMeasurementSubmission::from_presenter(
                SurfaceOrderBackendUsed::Cpu,
                TelemetrySubmission::Issued(7),
            ),
            SurfaceOrderMeasurementSubmission::Issued {
                backend: SurfaceOrderBackendUsed::Cpu,
                ticket: 7,
            },
        );
        assert_eq!(
            SurfaceOrderMeasurementSubmission::from_presenter(
                SurfaceOrderBackendUsed::Gpu,
                TelemetrySubmission::RingBusy,
            ),
            SurfaceOrderMeasurementSubmission::Unsampled {
                backend: SurfaceOrderBackendUsed::Gpu,
                reason: SurfaceOrderMeasurementUnsampledReason::RingBusy,
            },
        );
        assert_eq!(
            SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                SurfaceProjectedDrawExecution::Compact,
                TelemetrySubmission::SurfaceUnavailable,
            ),
            SurfaceProjectedDrawMeasurementSubmission::Unsampled {
                execution: SurfaceProjectedDrawExecution::Compact,
                reason: SurfaceProjectedDrawMeasurementUnsampledReason::SurfaceUnavailable,
            },
        );
        assert_eq!(
            SurfaceGpuProducerMeasurementSubmission::from_presenter(
                Some(SurfaceGpuOrderProducer::Preproject),
                TelemetrySubmission::Issued(11),
            ),
            SurfaceGpuProducerMeasurementSubmission::Issued {
                producer: SurfaceGpuOrderProducer::Preproject,
                ticket: 11,
            },
        );
        assert_eq!(
            SurfaceGpuProducerMeasurementSubmission::from_presenter(
                Some(SurfaceGpuOrderProducer::PostSort),
                TelemetrySubmission::RingBusy,
            ),
            SurfaceGpuProducerMeasurementSubmission::Unsampled {
                producer: SurfaceGpuOrderProducer::PostSort,
                reason: SurfaceGpuProducerMeasurementUnsampledReason::RingBusy,
            },
        );
        assert_eq!(
            SurfaceGpuProducerMeasurementSubmission::from_presenter(
                None,
                TelemetrySubmission::Issued(13),
            ),
            SurfaceGpuProducerMeasurementSubmission::NotRequested,
        );
    }

    #[test]
    fn adaptive_fallback_only_accepts_gpu_order_capability_failures() {
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::GpuOrderUnsupported
            )),
            Some(super::SurfaceAdaptiveGpuFailureReason::Unsupported)
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::GpuOrderPreparationRequired
            )),
            None
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::ResidentGpu(ResidentGpuError::GpuOrderOutOfMemory(
                    "injected".into()
                ))
            )),
            Some(super::SurfaceAdaptiveGpuFailureReason::OutOfMemory)
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::SurfaceOutOfMemory
            )),
            None
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::ResidentGpu(ResidentGpuError::GpuOrderInternal(
                    "injected".into()
                ))
            )),
            None
        );
    }

    #[test]
    fn paged_surface_counts_report_source_total_and_active_drawn() {
        assert_eq!(paged_surface_counts(279_199, 262_144), (279_199, 262_144));
    }

    #[derive(Debug, PartialEq)]
    struct RendererGeometrySnapshot {
        geometry_path: GeometryPath,
        scene_allocation: Option<usize>,
        resident_allocation: Option<usize>,
        exact_runtime_allocation: Option<usize>,
        positions_allocation: Option<usize>,
        world_covariances_allocation: Option<usize>,
        world_covariance_terms_allocation: Option<usize>,
        alpha_values_allocation: Option<usize>,
        spatial_pages_allocation: Option<usize>,
        preprocess_indices: Vec<u32>,
        last_stats: crate::FrameStats,
    }

    fn renderer_geometry_snapshot(renderer: &Renderer) -> RendererGeometrySnapshot {
        RendererGeometrySnapshot {
            geometry_path: renderer.geometry_path(),
            scene_allocation: renderer
                .scene()
                .map(|scene| std::ptr::from_ref(scene) as usize),
            resident_allocation: renderer
                .resident_scene()
                .map(|scene| std::ptr::from_ref(scene) as usize),
            exact_runtime_allocation: renderer
                .exact_offscreen_runtime
                .as_ref()
                .map(|runtime| std::ptr::from_ref(runtime) as usize),
            positions_allocation: renderer
                .positions()
                .map(|positions| positions.as_ptr() as usize),
            world_covariances_allocation: renderer
                .world_covariances()
                .map(|values| values.as_ptr() as usize),
            world_covariance_terms_allocation: renderer
                .direct_scene_cpu_inputs()
                .map(|(_, values, _)| values.as_ptr() as usize),
            alpha_values_allocation: renderer
                .direct_scene_cpu_inputs()
                .map(|(_, _, values)| values.as_ptr() as usize),
            spatial_pages_allocation: renderer
                .spatial_pages()
                .map(|pages| std::ptr::from_ref(pages) as usize),
            preprocess_indices: renderer.current_sorted_indices().to_vec(),
            last_stats: renderer.last_stats(),
        }
    }

    #[test]
    fn failed_presenter_prepare_rolls_renderer_back_to_working_path() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0), Vec3f::new(0.1, 0.0, 1.2)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[-3.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(scene).unwrap();
        assert_eq!(renderer.world_covariances().map(<[_]>::len), Some(2));

        let result = try_switch_renderer_geometry_path(
            &mut renderer,
            GeometryPath::PagedActiveAtlas,
            |prepared| {
                assert_eq!(prepared.geometry_path(), GeometryPath::PagedActiveAtlas);
                assert!(prepared.world_covariances().is_none());
                assert!(prepared.spatial_pages().is_some());
                Err(SurfacePresenterError::SurfaceConfigure(
                    "injected presenter allocation failure".into(),
                ))
            },
        );

        assert!(matches!(
            result,
            Err(SurfacePresenterError::SurfaceConfigure(message))
                if message == "injected presenter allocation failure"
        ));
        assert_eq!(renderer.geometry_path(), GeometryPath::SortedIndexDirect);
        assert_eq!(renderer.world_covariances().map(<[_]>::len), Some(2));
        assert!(renderer.spatial_pages().is_none());
    }

    #[test]
    fn direct_and_packed_live_switches_reject_before_renderer_mutation() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0), Vec3f::new(0.1, 0.0, 1.2)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[-3.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut direct = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        direct.load_scene(scene.clone()).unwrap();
        direct
            .build_sorted_indices(&Camera::default())
            .expect("establish Direct current frame state");
        let direct_before = renderer_geometry_snapshot(&direct);
        let mut direct_prepare_called = false;
        let direct_result =
            try_switch_renderer_geometry_path(&mut direct, GeometryPath::PackedAtlas, |_| {
                direct_prepare_called = true;
                Ok::<(), SurfacePresenterError>(())
            });
        assert!(matches!(
            direct_result,
            Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported)
        ));
        assert!(!direct_prepare_called);
        assert_eq!(renderer_geometry_snapshot(&direct), direct_before);
        assert!(matches!(
            try_switch_renderer_geometry_path(
                &mut direct,
                GeometryPath::SortedIndexDirect,
                |_| panic!("same-path Direct must not prepare presenter resources"),
            ),
            Ok(false)
        ));
        assert_eq!(renderer_geometry_snapshot(&direct), direct_before);

        let mut packed = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        packed.set_geometry_path(GeometryPath::PackedAtlas);
        packed.load_scene(scene).unwrap();
        packed
            .finish_surface_upload_handoff(GeometryPath::PackedAtlas)
            .unwrap();
        let packed_before = renderer_geometry_snapshot(&packed);
        let mut packed_prepare_called = false;
        let packed_result =
            try_switch_renderer_geometry_path(&mut packed, GeometryPath::SortedIndexDirect, |_| {
                packed_prepare_called = true;
                Ok::<(), SurfacePresenterError>(())
            });
        assert!(matches!(
            packed_result,
            Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported)
        ));
        assert!(!packed_prepare_called);
        assert_eq!(renderer_geometry_snapshot(&packed), packed_before);
        assert!(matches!(
            try_switch_renderer_geometry_path(&mut packed, GeometryPath::PackedAtlas, |_| {
                panic!("same-path Packed must not prepare presenter resources")
            }),
            Ok(false)
        ));
        assert_eq!(renderer_geometry_snapshot(&packed), packed_before);
    }

    fn feed_cpu_bootstrap(policy: &mut AdaptiveOrderPolicy, sample_ms: f32) {
        policy.reset(AdaptiveMetric::OrderOnly);
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            let choice = policy.choose_refresh_backend();
            assert_eq!(choice.backend, SurfaceOrderBackendUsed::Cpu);
            assert_eq!(choice.sample, Some(AdaptiveSampleKind::CpuBootstrap));
            policy.complete_synchronous_sample(choice, sample_ms);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        for _ in 0..ADAPTIVE_INITIAL_PROBE_DELAY {
            let choice = policy.choose_refresh_backend();
            assert_eq!(choice.backend, SurfaceOrderBackendUsed::Cpu);
            assert_eq!(choice.sample, None);
        }
    }

    #[test]
    fn reapplying_the_same_raster_plan_preserves_adaptive_history() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        assert!(!reset_adaptive_for_raster_transition(
            &mut policy,
            crate::SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            crate::SurfaceRasterExecutionPlan::ProjectedQuadsExact,
        ));
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        assert!(reset_adaptive_for_raster_transition(
            &mut policy,
            crate::SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            crate::SurfaceRasterExecutionPlan::GlobalQuads,
        ));
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuLearning);
        assert_eq!(policy.metric(), adaptive_primary_metric());
    }

    #[test]
    fn first_changed_then_cached_order_preserves_one_projected_grace_turn() {
        let mut lane = AdaptiveProjectedDrawPolicy::default();
        let formal = lane.choose(true);
        let projected_owner = AdaptiveProbeOwner::ProjectedCpu;

        // On the first required sort, the formal projected choice cannot be
        // measured yet. It nevertheless owns one grace turn and is retained
        // as pending so a static second frame can issue the exact ticket.
        assert!(!projected_formal_sample_requested(formal, true));
        assert!(defer_projected_formal_choice(formal, true));
        assert!(projected_probe_claims_owner(formal, true, false));
        let owner = arbitrate_new_probe_owner(None, true, true, projected_owner);
        assert_eq!(owner, Some(projected_owner));

        // The retained choice becomes eligible when the next frame reuses the
        // exact same order. Projected remains owner and the blocked order
        // sample cannot contaminate its timing cohort.
        assert!(projected_formal_sample_requested(formal, false));
        assert!(projected_probe_claims_owner(formal, false, true));
        assert_eq!(owner, Some(projected_owner));
        assert!(!projected_policy_can_sample(
            Some(AdaptiveProbeOwner::Order),
            projected_owner,
            true,
        ));
    }

    #[test]
    fn repeated_changed_orders_release_projected_grace_and_advance_order_learning() {
        let mut projected = AdaptiveProjectedDrawPolicy::default();
        let mut formal = projected.choose(true);
        let projected_owner = AdaptiveProbeOwner::ProjectedCpu;
        let mut pending_projected_choice = Some(formal);

        let mut owner = arbitrate_new_probe_owner(
            None,
            true,
            projected_probe_claims_owner(formal, true, false),
            projected_owner,
        );
        assert_eq!(owner, Some(projected_owner));

        // The same pending formal choice sees a second order change. Its one
        // grace turn is exhausted, so production releases Projected and Order
        // receives the still-unconsumed bootstrap sample.
        assert!(defer_projected_formal_choice(formal, true));
        if owner == Some(projected_owner) && pending_projected_choice.is_some() {
            owner = None;
            pending_projected_choice = None;
            formal.sample = None;
        }
        assert!(pending_projected_choice.is_none());
        assert!(formal.sample.is_none());
        assert!(!projected_probe_claims_owner(formal, true, true));
        owner = arbitrate_new_probe_owner(owner, true, false, projected_owner);
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));

        // Once Order owns the cohort, a finite number of successful bootstrap
        // receipts necessarily leaves CpuLearning instead of livelocking.
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            let choice = order.choose_refresh_backend();
            assert_eq!(choice.sample, Some(AdaptiveSampleKind::CpuBootstrap));
            order.complete_synchronous_sample(choice, 10.0);
        }
        assert_eq!(order.state(), SurfaceAdaptiveState::CpuStable);
    }

    #[test]
    fn yielded_projected_choice_stays_unsampled_until_order_cohort_releases() {
        let mut projected = AdaptiveProjectedDrawPolicy::default();
        let formal = projected.choose(true);
        let projected_owner = AdaptiveProbeOwner::ProjectedCpu;

        // refresh -> refresh: the first frame grants grace; the second yields
        // to Order and removes the session-level pending ticket.
        let mut owner = Some(projected_owner);
        let mut pending_projected_choice = Some(formal);
        let mut yielded = pending_projected_choice.expect("grace choice");
        if owner == Some(projected_owner)
            && defer_projected_formal_choice(yielded, true)
            && pending_projected_choice.is_some()
        {
            owner = None;
            pending_projected_choice = None;
            yielded.sample = None;
        }
        owner = arbitrate_new_probe_owner(owner, true, false, projected_owner);
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));
        assert!(pending_projected_choice.is_none());
        assert!(yielded.sample.is_none());

        // A following stable frame cannot steal ownership while the Order
        // ticket from the second refresh is still pending.
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        let order_choice = order.choose_refresh_backend();
        order.register_pending_sample(order_choice, 77);
        assert!(!order_probe_owner_should_yield(
            owner,
            order.has_pending_sample(),
            false,
            false,
        ));
        let held = projected.held_choice();
        assert!(held.sample.is_none());
        assert!(!projected_probe_claims_owner(held, false, false));
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));
        assert!(projected.pending_sample().is_none());

        // Once that ticket is terminal, Order has no work on a stable frame
        // and yields without losing its CpuLearning phase or first sample.
        assert!(order.complete_pending_sample(SurfaceOrderBackendUsed::Cpu, 77, 10.0,));
        assert!(order_probe_owner_should_yield(
            owner,
            order.has_pending_sample(),
            false,
            false,
        ));
        owner = None;
        assert_eq!(order.state(), SurfaceAdaptiveState::CpuLearning);

        // Projected now reissues the exact same unconsumed bootstrap kind and
        // the cached order makes its formal ticket eligible.
        let reissued = projected.choose(true);
        assert_eq!(reissued.sample, formal.sample);
        assert!(projected_formal_sample_requested(reissued, false));
        owner = arbitrate_new_probe_owner(
            owner,
            false,
            projected_probe_claims_owner(reissued, false, false),
            projected_owner,
        );
        assert_eq!(owner, Some(projected_owner));
    }

    #[test]
    fn projected_transition_warmup_delays_order_once_without_claiming_owner() {
        let transition_warmup = ProjectedAdaptiveChoice {
            execution: SurfaceProjectedDrawExecution::Compact,
            sample: Some(ProjectedAdaptiveSampleKind::TransitionWarmup),
        };
        assert!(!projected_formal_sample_requested(transition_warmup, false));
        assert!(!projected_probe_claims_owner(
            transition_warmup,
            false,
            false,
        ));
        assert_eq!(
            arbitrate_new_probe_owner(None, true, false, AdaptiveProbeOwner::ProjectedCpu),
            Some(AdaptiveProbeOwner::Order),
        );
    }

    #[test]
    fn projected_formal_ticket_requires_a_cached_order_and_forced_reprojection() {
        let mut lane = AdaptiveProjectedDrawPolicy::default();
        let formal = lane.choose(true);
        for (refresh_sort, upload_order, actual_sort_refreshed) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let changed =
                projected_order_changed(refresh_sort, upload_order, actual_sort_refreshed);
            assert!(changed);
            assert!(!projected_formal_sample_requested(formal, changed));
            assert!(defer_projected_formal_choice(formal, changed));
        }
        let cached = projected_order_changed(false, false, false);
        assert!(projected_formal_sample_requested(formal, cached));
        assert!(!defer_projected_formal_choice(formal, cached));

        let transition_warmup = ProjectedAdaptiveChoice {
            execution: SurfaceProjectedDrawExecution::Compact,
            sample: Some(ProjectedAdaptiveSampleKind::TransitionWarmup),
        };
        assert_eq!(
            transition_warmup.sample,
            Some(ProjectedAdaptiveSampleKind::TransitionWarmup)
        );
        assert!(!projected_formal_sample_requested(transition_warmup, false));
        assert!(!defer_projected_formal_choice(transition_warmup, true));
    }

    #[test]
    fn deferred_cpu_upload_does_not_block_cached_gpu_projected_ticket() {
        let second_gpu_plan = super::SurfaceFramePlan {
            refresh_sort: false,
            // The first GPU frame intentionally leaves this dirty for a
            // future CPU switch; it is not part of the GPU order identity.
            upload_order: true,
        };
        let mut lane = AdaptiveProjectedDrawPolicy::default();
        let formal = lane.choose(true);
        let changed = gpu_projected_order_changed(second_gpu_plan.refresh_sort, false);
        assert!(!changed);
        assert!(projected_formal_sample_requested(formal, changed));
        assert!(!defer_projected_formal_choice(formal, changed));
    }

    #[test]
    fn projected_unsampled_submission_does_not_advance_the_policy() {
        let mut policy = AdaptiveProjectedDrawPolicy::default();
        let first = policy.choose(true);
        assert_eq!(
            first.sample,
            Some(ProjectedAdaptiveSampleKind::CandidateBootstrap),
        );
        for submission in [
            TelemetrySubmission::RingBusy,
            TelemetrySubmission::SurfaceUnavailable,
        ] {
            assert!(matches!(
                SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                    first.execution,
                    submission,
                ),
                SurfaceProjectedDrawMeasurementSubmission::Unsampled { .. }
            ));
            assert_eq!(policy.choose(true), first);
        }
    }

    #[test]
    fn projected_incumbent_change_resets_order_only_at_a_clear_owner_boundary() {
        assert!(should_reset_order_for_projected_incumbent_change(
            SurfaceOrderBackend::Adaptive,
            SurfaceProjectedDrawPolicy::Adaptive,
            AdaptiveMetric::FrameCompletion,
            false,
            true,
            true,
        ));
        assert!(!should_reset_order_for_projected_incumbent_change(
            SurfaceOrderBackend::Adaptive,
            SurfaceProjectedDrawPolicy::Adaptive,
            AdaptiveMetric::FrameCompletion,
            true,
            true,
            true,
        ));
        assert!(!should_reset_order_for_projected_incumbent_change(
            SurfaceOrderBackend::Adaptive,
            SurfaceProjectedDrawPolicy::Adaptive,
            AdaptiveMetric::FrameCompletion,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn forced_projected_policy_validation_is_transactional_and_idempotent() {
        assert!(matches!(
            validate_projected_draw_policy_transition(
                SurfaceProjectedDrawPolicy::Adaptive,
                SurfaceProjectedDrawPolicy::Adaptive,
                false,
            ),
            Ok(false),
        ));
        assert!(matches!(
            validate_projected_draw_policy_transition(
                SurfaceProjectedDrawPolicy::Candidate,
                SurfaceProjectedDrawPolicy::Compact,
                false,
            ),
            Err(SurfacePresenterError::ProjectedCompactionUnsupported),
        ));
        assert!(matches!(
            validate_projected_draw_policy_transition(
                SurfaceProjectedDrawPolicy::Candidate,
                SurfaceProjectedDrawPolicy::Adaptive,
                false,
            ),
            Ok(true),
        ));
    }

    #[test]
    fn preproject_selector_is_idempotent_and_rejects_every_incompatible_context() {
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::Preproject,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::SortedIndexDirect,
                SurfaceRasterExecutionPlan::GlobalQuads,
                SurfaceProjectedDrawPolicy::Candidate,
            ),
            Ok(false),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::SortedIndexDirect,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Err(SurfacePresenterError::PreprojectProducerIncompatible),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::GlobalQuads,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Err(SurfacePresenterError::PreprojectProducerIncompatible),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Adaptive,
            ),
            Err(SurfacePresenterError::PreprojectProducerIncompatible),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Ok(true),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::Preproject,
                SurfaceGpuOrderProducer::PostSort,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Ok(true),
        ));
    }

    #[test]
    fn producer_measurements_require_the_isolated_forced_compact_context() {
        assert!(gpu_producer_measurement_context_is_valid(
            GeometryPath::PackedAtlas,
            SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            SurfaceProjectedDrawPolicy::Compact,
        ));
        assert!(!gpu_producer_measurement_context_is_valid(
            GeometryPath::PackedAtlas,
            SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            SurfaceProjectedDrawPolicy::Adaptive,
        ));
        assert!(!gpu_producer_measurement_context_is_valid(
            GeometryPath::PackedAtlas,
            SurfaceRasterExecutionPlan::GlobalQuads,
            SurfaceProjectedDrawPolicy::Compact,
        ));
    }

    #[test]
    fn producer_measurement_control_reports_committed_state_across_transitions_and_errors() {
        let mut control = SurfaceGpuProducerMeasurementControl::default();
        assert!(!control.enabled());
        assert!(matches!(control.transition(false, false), Ok(false)));
        assert!(!control.enabled());

        assert!(matches!(
            control.transition(true, false),
            Err(SurfacePresenterError::PreprojectProducerIncompatible)
        ));
        assert!(!control.enabled());

        assert!(matches!(control.transition(true, true), Ok(true)));
        assert!(control.enabled());
        assert!(matches!(control.transition(true, false), Ok(false)));
        assert!(control.enabled());

        assert!(matches!(control.transition(false, false), Ok(true)));
        assert!(!control.enabled());
    }

    #[test]
    fn resetting_order_policy_does_not_clear_either_projected_lane() {
        let mut order = AdaptiveOrderPolicy::default();
        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        let cpu_choice = cpu.choose(true);
        let gpu_choice = gpu.choose(true);
        order.reset(AdaptiveMetric::FrameCompletion);
        assert_eq!(cpu.choose(true), cpu_choice);
        assert_eq!(gpu.choose(true), gpu_choice);
    }

    #[test]
    fn gpu_producer_measurement_graph_transition_discards_all_pending_learning() {
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        let order_choice = order.choose_refresh_backend();
        order.register_pending_sample(order_choice, 41);

        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let cpu_choice = cpu.choose(true);
        cpu.register_pending_sample(SurfaceOrderBackendUsed::Cpu, cpu_choice, 42);
        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        let gpu_choice = gpu.choose(true);
        gpu.register_pending_sample(SurfaceOrderBackendUsed::Gpu, gpu_choice, 43);
        let mut owner = Some(AdaptiveProbeOwner::Order);
        let mut blocked = Some(order_choice);

        reset_adaptive_for_gpu_producer_measurement_transition(
            &mut order,
            &mut cpu,
            &mut gpu,
            &mut owner,
            &mut blocked,
        );

        assert_eq!(order.state(), SurfaceAdaptiveState::CpuLearning);
        assert!(!order.has_pending_sample());
        assert!(cpu.pending_sample().is_none());
        assert!(gpu.pending_sample().is_none());
        assert_eq!(
            cpu.state(),
            SurfaceProjectedDrawAdaptiveState::CandidateLearning
        );
        assert_eq!(
            gpu.state(),
            SurfaceProjectedDrawAdaptiveState::CandidateLearning
        );
        assert!(owner.is_none());
        assert!(blocked.is_none());
    }
}
