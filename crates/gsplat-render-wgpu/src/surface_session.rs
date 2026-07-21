#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::Vec3f;
use gsplat_core::{Camera, FrameStats};
#[cfg(not(target_arch = "wasm32"))]
use gsplat_sort::CpuSortBackend;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
};

use crate::{GeometryPath, Renderer, RendererError, SurfacePresenter, timer_elapsed_ms, timer_now};

const DEFAULT_SURFACE_SORT_INTERVAL: u32 = 2;
/// Maximum number of camera revisions an asynchronously produced order may lag
/// behind the frame that consumes it. Older results are dropped; if the
/// displayed order reaches this bound before a fresh result is ready, the
/// session performs a synchronous refresh rather than allowing unbounded lag.
const MAX_ASYNC_SORT_REVISION_LAG: u64 = 2;
const MAX_ASYNC_SORT_ROTATION_DELTA_RADIANS: f32 = 0.01;
const MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION: f32 = 0.02;
const ADAPTIVE_CPU_BOOTSTRAP_SAMPLES: u32 = 6;
const ADAPTIVE_INITIAL_PROBE_DELAY: u32 = 4;
const ADAPTIVE_PROBE_SAMPLES: u32 = 4;
const ADAPTIVE_REPROBE_INTERVAL: u32 = 48;
const ADAPTIVE_GPU_PROMOTION_RATIO: f32 = 0.88;
const ADAPTIVE_CPU_PROMOTION_RATIO: f32 = 0.92;
const ADAPTIVE_EMERGENCY_DEMOTION_RATIO: f32 = 1.25;
const ADAPTIVE_GPU_FAILURE_COOLDOWN: u32 = 96;

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

/// Backend that actually supplied the order presented by one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceOrderBackendUsed {
    Cpu,
    Gpu,
}

/// Coarse policy state retained in benchmark telemetry. Thresholds are
/// exploration controls rather than fixed CPU/GPU crossover decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceAdaptiveState {
    #[default]
    Disabled,
    CpuLearning,
    CpuStable,
    GpuProbe,
    GpuStable,
    CpuProbe,
    Cooldown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdaptivePhase {
    CpuLearning,
    CpuStable,
    GpuProbe { remaining: u32, skip_warmup: bool },
    GpuStable,
    CpuProbe { remaining: u32 },
    Cooldown { remaining: u32 },
}

#[derive(Debug, Clone, Copy)]
struct RollingEstimate {
    samples: [f32; 16],
    len: usize,
    cursor: usize,
}

impl Default for RollingEstimate {
    fn default() -> Self {
        Self {
            samples: [0.0; 16],
            len: 0,
            cursor: 0,
        }
    }
}

impl RollingEstimate {
    fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
    }

    fn push(&mut self, sample_ms: f32) {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return;
        }
        self.samples[self.cursor] = sample_ms;
        self.cursor = (self.cursor + 1) % self.samples.len();
        self.len = self.len.saturating_add(1).min(self.samples.len());
    }

    fn p75(&self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.samples;
        sorted[..self.len].sort_by(f32::total_cmp);
        let index = (self.len - 1) * 3 / 4;
        Some(sorted[index])
    }
}

#[derive(Debug, Default)]
struct AdaptiveTimingEstimate {
    refresh: RollingEstimate,
    reuse: RollingEstimate,
}

impl AdaptiveTimingEstimate {
    fn clear(&mut self) {
        self.refresh.clear();
        self.reuse.clear();
    }

    fn push(&mut self, sample_ms: f32, refresh: bool) {
        if refresh {
            self.refresh.push(sample_ms);
        } else {
            self.reuse.push(sample_ms);
        }
    }

    /// Estimates the end-to-end cost at the configured cadence. Keeping the
    /// refresh and reuse tails separate prevents a costly refresh from falling
    /// below p75 merely because the sort interval is four or greater.
    fn cadence_score(&self, sort_interval: u32) -> Option<f32> {
        match (self.refresh.p75(), self.reuse.p75()) {
            (Some(refresh), Some(reuse)) if sort_interval > 1 => {
                let reuse_weight = sort_interval.saturating_sub(1) as f32;
                Some((refresh + reuse * reuse_weight) / sort_interval as f32)
            }
            (Some(refresh), _) => Some(refresh),
            (None, Some(reuse)) => Some(reuse),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Default)]
struct AdaptiveOrderPolicy {
    phase: Option<AdaptivePhase>,
    cpu: AdaptiveTimingEstimate,
    gpu: AdaptiveTimingEstimate,
    refreshes_since_probe: u32,
    cpu_bootstrap_refreshes: u32,
    next_gpu_probe_after: u32,
    gpu_initialized: bool,
}

impl AdaptiveOrderPolicy {
    fn reset(&mut self) {
        *self = Self {
            phase: Some(AdaptivePhase::CpuLearning),
            next_gpu_probe_after: ADAPTIVE_INITIAL_PROBE_DELAY,
            ..Self::default()
        };
    }

    fn state(&self) -> SurfaceAdaptiveState {
        match self.phase {
            None => SurfaceAdaptiveState::Disabled,
            Some(AdaptivePhase::CpuLearning) => SurfaceAdaptiveState::CpuLearning,
            Some(AdaptivePhase::CpuStable) => SurfaceAdaptiveState::CpuStable,
            Some(AdaptivePhase::GpuProbe { .. }) => SurfaceAdaptiveState::GpuProbe,
            Some(AdaptivePhase::GpuStable) => SurfaceAdaptiveState::GpuStable,
            Some(AdaptivePhase::CpuProbe { .. }) => SurfaceAdaptiveState::CpuProbe,
            Some(AdaptivePhase::Cooldown { .. }) => SurfaceAdaptiveState::Cooldown,
        }
    }

    fn choose_refresh_backend(&mut self) -> SurfaceOrderBackendUsed {
        let phase = self.phase.get_or_insert(AdaptivePhase::CpuLearning);
        match *phase {
            AdaptivePhase::CpuLearning | AdaptivePhase::Cooldown { .. } => {
                SurfaceOrderBackendUsed::Cpu
            }
            AdaptivePhase::CpuStable => {
                if self.cpu.refresh.len >= ADAPTIVE_CPU_BOOTSTRAP_SAMPLES as usize
                    && self.refreshes_since_probe >= self.next_gpu_probe_after
                {
                    self.gpu.clear();
                    *phase = AdaptivePhase::GpuProbe {
                        remaining: ADAPTIVE_PROBE_SAMPLES,
                        skip_warmup: !self.gpu_initialized,
                    };
                    SurfaceOrderBackendUsed::Gpu
                } else {
                    SurfaceOrderBackendUsed::Cpu
                }
            }
            AdaptivePhase::GpuProbe { .. } | AdaptivePhase::GpuStable => {
                if matches!(*phase, AdaptivePhase::GpuStable)
                    && self.refreshes_since_probe >= ADAPTIVE_REPROBE_INTERVAL
                {
                    self.cpu.clear();
                    *phase = AdaptivePhase::CpuProbe {
                        remaining: ADAPTIVE_PROBE_SAMPLES,
                    };
                    SurfaceOrderBackendUsed::Cpu
                } else {
                    SurfaceOrderBackendUsed::Gpu
                }
            }
            AdaptivePhase::CpuProbe { .. } => SurfaceOrderBackendUsed::Cpu,
        }
    }

    fn observe_frame(
        &mut self,
        backend: SurfaceOrderBackendUsed,
        frame_ms: f32,
        refresh: bool,
        sort_interval: u32,
    ) {
        let Some(phase) = self.phase else {
            return;
        };
        let skip_sample = matches!(
            phase,
            AdaptivePhase::GpuProbe {
                skip_warmup: true,
                ..
            }
        );
        if !skip_sample {
            match backend {
                SurfaceOrderBackendUsed::Cpu => self.cpu.push(frame_ms, refresh),
                SurfaceOrderBackendUsed::Gpu => self.gpu.push(frame_ms, refresh),
            }
        }
        match phase {
            AdaptivePhase::CpuLearning => {
                if refresh {
                    self.cpu_bootstrap_refreshes = self.cpu_bootstrap_refreshes.saturating_add(1);
                }
                if self.cpu_bootstrap_refreshes >= ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
                    self.phase = Some(AdaptivePhase::CpuStable);
                    self.refreshes_since_probe = 0;
                }
            }
            AdaptivePhase::CpuStable => {
                if refresh {
                    self.refreshes_since_probe = self.refreshes_since_probe.saturating_add(1);
                }
            }
            AdaptivePhase::GpuProbe {
                mut remaining,
                skip_warmup,
            } => {
                self.gpu_initialized = true;
                if skip_warmup && refresh {
                    self.phase = Some(AdaptivePhase::GpuProbe {
                        remaining,
                        skip_warmup: false,
                    });
                    return;
                }
                if skip_warmup {
                    return;
                }
                if !refresh {
                    return;
                }
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    let gpu_wins = match (
                        self.cpu.cadence_score(sort_interval),
                        self.gpu.cadence_score(sort_interval),
                    ) {
                        (Some(cpu), Some(gpu)) => gpu < cpu * ADAPTIVE_GPU_PROMOTION_RATIO,
                        _ => false,
                    };
                    self.phase = Some(if gpu_wins {
                        self.next_gpu_probe_after = ADAPTIVE_REPROBE_INTERVAL;
                        AdaptivePhase::GpuStable
                    } else {
                        self.next_gpu_probe_after =
                            if self.next_gpu_probe_after < ADAPTIVE_REPROBE_INTERVAL {
                                ADAPTIVE_REPROBE_INTERVAL
                            } else {
                                self.next_gpu_probe_after.saturating_mul(2).min(384)
                            };
                        AdaptivePhase::CpuStable
                    });
                    self.refreshes_since_probe = 0;
                } else {
                    self.phase = Some(AdaptivePhase::GpuProbe {
                        remaining,
                        skip_warmup: false,
                    });
                }
            }
            AdaptivePhase::GpuStable => {
                if refresh {
                    self.refreshes_since_probe = self.refreshes_since_probe.saturating_add(1);
                }
                if backend == SurfaceOrderBackendUsed::Gpu
                    && matches!(
                        (
                            self.cpu.cadence_score(sort_interval),
                            self.gpu.cadence_score(sort_interval),
                        ),
                        (Some(cpu), Some(gpu))
                            if gpu > cpu * ADAPTIVE_EMERGENCY_DEMOTION_RATIO
                    )
                {
                    self.phase = Some(AdaptivePhase::CpuStable);
                    self.refreshes_since_probe = 0;
                }
            }
            AdaptivePhase::CpuProbe { mut remaining } => {
                if !refresh {
                    return;
                }
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    let cpu_wins = match (
                        self.cpu.cadence_score(sort_interval),
                        self.gpu.cadence_score(sort_interval),
                    ) {
                        (Some(cpu), Some(gpu)) => cpu < gpu * ADAPTIVE_CPU_PROMOTION_RATIO,
                        _ => true,
                    };
                    self.phase = Some(if cpu_wins {
                        AdaptivePhase::CpuStable
                    } else {
                        AdaptivePhase::GpuStable
                    });
                    self.refreshes_since_probe = 0;
                } else {
                    self.phase = Some(AdaptivePhase::CpuProbe { remaining });
                }
            }
            AdaptivePhase::Cooldown { mut remaining } => {
                if !refresh {
                    return;
                }
                remaining = remaining.saturating_sub(1);
                self.phase = Some(if remaining == 0 {
                    self.refreshes_since_probe = 0;
                    self.next_gpu_probe_after = 0;
                    AdaptivePhase::CpuStable
                } else {
                    AdaptivePhase::Cooldown { remaining }
                });
            }
        }
    }

    fn gpu_failed(&mut self) {
        self.phase = Some(AdaptivePhase::Cooldown {
            remaining: ADAPTIVE_GPU_FAILURE_COOLDOWN,
        });
        self.refreshes_since_probe = 0;
        self.next_gpu_probe_after = ADAPTIVE_GPU_FAILURE_COOLDOWN;
        self.gpu.clear();
    }
}

impl SurfaceSortSchedule {
    pub const fn interval(self) -> u32 {
        match self {
            Self::Interval(interval) | Self::AsyncLatest { interval } => interval,
        }
    }
}

fn async_schedule_threshold(sort_interval: u32) -> u32 {
    sort_interval.saturating_sub(1).max(1)
}

#[cfg(not(target_arch = "wasm32"))]
struct SurfaceAsyncSorter {
    request_tx: SyncSender<Option<(Camera, u64)>>,
    result_rx: Receiver<Result<AsyncSortResult, RendererError>>,
    worker: Option<JoinHandle<()>>,
    in_flight: bool,
}

#[cfg(not(target_arch = "wasm32"))]
struct AsyncSortResult {
    indices: Vec<u32>,
    preprocess_ms: f32,
    sort_ms: f32,
    camera_revision: u64,
    camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
impl SurfaceAsyncSorter {
    fn new(renderer: &Renderer) -> Result<Self, RendererError> {
        let scene = renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
        let positions: Arc<[Vec3f]> = Arc::from(scene.positions.clone().into_boxed_slice());
        let (request_tx, request_rx) = sync_channel::<Option<(Camera, u64)>>(1);
        let (result_tx, result_rx) = sync_channel(1);
        let worker = thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let Some((camera, camera_revision)) = request else {
                    break;
                };
                if result_tx
                    .send(sort_positions_for_camera(
                        &positions,
                        camera,
                        camera_revision,
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
        Ok(Self {
            request_tx,
            result_rx,
            worker: Some(worker),
            in_flight: false,
        })
    }

    fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    fn poll_result(&mut self) -> Option<Result<AsyncSortResult, RendererError>> {
        if !self.in_flight {
            return None;
        }
        match self.result_rx.try_recv() {
            Ok(result) => {
                self.in_flight = false;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.in_flight = false;
                Some(Err(RendererError::SurfaceWorker))
            }
        }
    }

    fn start(&mut self, camera: Camera, camera_revision: u64) {
        if self.in_flight {
            return;
        }
        if self
            .request_tx
            .try_send(Some((camera, camera_revision)))
            .is_ok()
        {
            self.in_flight = true;
        }
    }

    fn drain(&mut self) {
        let _ = self.request_tx.send(None);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.in_flight = false;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SurfaceAsyncSorter {
    fn drop(&mut self) {
        self.drain();
    }
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
    /// CPU ordering phases are populated for CPU frames. The GPU-order path
    /// currently reports those unavailable phase fields as zero; use
    /// `timings.frame_wall_ms` plus `order_backend` for backend comparisons.
    pub stats: FrameStats,
    pub timings: SurfaceFrameTimings,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceFramePlan {
    refresh_sort: bool,
    upload_order: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceFrameState {
    camera_dirty: bool,
    force_sort: bool,
    order_upload_dirty: bool,
    camera_changes_since_sort: u32,
}

impl Default for SurfaceFrameState {
    fn default() -> Self {
        Self {
            camera_dirty: true,
            force_sort: true,
            order_upload_dirty: true,
            camera_changes_since_sort: 0,
        }
    }
}

impl SurfaceFrameState {
    fn mark_camera_changed(&mut self) {
        self.camera_dirty = true;
        self.camera_changes_since_sort = self.camera_changes_since_sort.saturating_add(1);
    }

    fn force_sort(&mut self) {
        self.force_sort = true;
        self.order_upload_dirty = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn mark_external_order(&mut self, camera_changes_since_sort: u32) {
        self.camera_dirty = camera_changes_since_sort > 0;
        self.force_sort = false;
        self.order_upload_dirty = true;
        self.camera_changes_since_sort = camera_changes_since_sort;
    }

    fn plan(self, has_order: bool, sort_interval: u32) -> SurfaceFramePlan {
        let interval = sort_interval.max(1);
        let refresh_sort = self.force_sort
            || !has_order
            || (self.camera_dirty && self.camera_changes_since_sort >= interval);
        SurfaceFramePlan {
            refresh_sort,
            upload_order: self.order_upload_dirty || refresh_sort,
        }
    }

    fn finish_frame(&mut self, plan: SurfaceFramePlan, order_uploaded: bool) {
        self.camera_dirty = false;
        self.force_sort = false;
        if plan.refresh_sort {
            self.camera_changes_since_sort = 0;
        }
        if order_uploaded {
            self.order_upload_dirty = false;
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
    presenter: SurfacePresenter,
    camera: Camera,
    sort_interval: u32,
    order_backend: SurfaceOrderBackend,
    presented_order_backend: SurfaceOrderBackendUsed,
    gpu_order_initialized: bool,
    adaptive_policy: AdaptiveOrderPolicy,
    camera_revision: u64,
    applied_order_revision: u64,
    applied_order_camera: Camera,
    async_sort_translation_limit: f32,
    frame_state: SurfaceFrameState,
    last_stats: FrameStats,
    #[cfg(not(target_arch = "wasm32"))]
    async_sort_enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    async_sorter: Option<SurfaceAsyncSorter>,
}

fn try_switch_renderer_geometry_path<Error>(
    renderer: &mut Renderer,
    target: GeometryPath,
    prepare_presenter: impl FnOnce(&Renderer) -> Result<(), Error>,
) -> Result<bool, Error> {
    let previous = renderer.geometry_path();
    if previous == target {
        return Ok(false);
    }

    renderer.set_geometry_path(target);
    if let Err(error) = prepare_presenter(renderer) {
        renderer.set_geometry_path(previous);
        return Err(error);
    }
    Ok(true)
}

impl SurfaceRenderSession {
    pub fn new(
        renderer: Renderer,
        presenter: SurfacePresenter,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if renderer.scene().is_none() {
            return Err(RendererError::SceneNotLoaded);
        }
        if renderer.geometry_path() != presenter.geometry_path() {
            return Err(RendererError::InvalidConfig);
        }
        let scene = renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for position in &scene.positions {
            min[0] = min[0].min(position.x);
            min[1] = min[1].min(position.y);
            min[2] = min[2].min(position.z);
            max[0] = max[0].max(position.x);
            max[1] = max[1].max(position.y);
            max[2] = max[2].max(position.z);
        }
        let diagonal =
            ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2))
                .sqrt();
        let async_sort_translation_limit =
            (diagonal * MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION).max(1e-4);
        Ok(Self {
            renderer,
            presenter,
            camera,
            sort_interval: DEFAULT_SURFACE_SORT_INTERVAL,
            order_backend: SurfaceOrderBackend::Cpu,
            presented_order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_order_initialized: false,
            adaptive_policy: AdaptiveOrderPolicy::default(),
            camera_revision: 0,
            applied_order_revision: 0,
            applied_order_camera: camera,
            async_sort_translation_limit,
            frame_state: SurfaceFrameState::default(),
            last_stats: FrameStats::zero(),
            #[cfg(not(target_arch = "wasm32"))]
            async_sort_enabled: false,
            #[cfg(not(target_arch = "wasm32"))]
            async_sorter: None,
        })
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn geometry_path(&self) -> GeometryPath {
        self.renderer.geometry_path()
    }

    /// Switches the shared renderer and presenter to a different geometry
    /// path (experimental A/B benchmark knob; default remains
    /// [`GeometryPath::SortedIndexDirect`]).
    pub fn set_geometry_path(&mut self, path: GeometryPath) -> Result<(), RendererError> {
        if path != GeometryPath::SortedIndexDirect && self.order_backend != SurfaceOrderBackend::Cpu
        {
            return Err(RendererError::InvalidConfig);
        }
        let changed = try_switch_renderer_geometry_path(&mut self.renderer, path, |renderer| {
            self.presenter.set_geometry_path(path, renderer)
        })?;
        if !changed {
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if path == GeometryPath::PagedActiveAtlas {
            self.disable_async_sort();
        }
        self.gpu_order_initialized = false;
        self.adaptive_policy.reset();
        self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
        self.frame_state.force_sort();
        Ok(())
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn set_camera(&mut self, camera: Camera) -> Result<(), RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if self.camera != camera {
            self.camera = camera;
            self.camera_revision = self.camera_revision.wrapping_add(1);
            self.frame_state.mark_camera_changed();
        }
        Ok(())
    }

    pub fn surface_size(&self) -> (u32, u32) {
        self.presenter.surface_size()
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        self.presenter.resize(width, height);
        let (surface_width, surface_height) = self.presenter.surface_size();
        self.renderer.set_size(surface_width, surface_height)
    }

    pub fn sort_interval(&self) -> u32 {
        self.sort_interval
    }

    pub const fn order_backend(&self) -> SurfaceOrderBackend {
        self.order_backend
    }

    pub fn set_order_backend(&mut self, backend: SurfaceOrderBackend) -> Result<(), RendererError> {
        if self.order_backend == backend {
            return Ok(());
        }
        if backend != SurfaceOrderBackend::Cpu
            && self.geometry_path() != GeometryPath::SortedIndexDirect
        {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if backend != SurfaceOrderBackend::Cpu && self.async_sort_enabled {
            return Err(RendererError::InvalidConfig);
        }
        let gpu_prepare_error = if backend == SurfaceOrderBackend::Cpu {
            None
        } else {
            self.presenter.prepare_direct_gpu_order().err()
        };
        let gpu_prepare_failed = match (backend, gpu_prepare_error) {
            (SurfaceOrderBackend::Gpu, Some(error)) => return Err(error.into()),
            (SurfaceOrderBackend::Adaptive, Some(_)) => true,
            _ => false,
        };
        self.order_backend = backend;
        self.adaptive_policy.reset();
        if backend == SurfaceOrderBackend::Adaptive {
            if gpu_prepare_failed {
                self.adaptive_policy.gpu_failed();
            } else {
                // Pipeline/buffer creation has already happened outside frame
                // measurement; the first probe no longer needs a compile-jank
                // sample exclusion.
                self.adaptive_policy.gpu_initialized = true;
            }
        }
        self.frame_state.force_sort();
        Ok(())
    }

    pub fn sort_schedule(&self) -> SurfaceSortSchedule {
        #[cfg(not(target_arch = "wasm32"))]
        if self.async_sort_enabled {
            return SurfaceSortSchedule::AsyncLatest {
                interval: self.sort_interval,
            };
        }
        SurfaceSortSchedule::Interval(self.sort_interval)
    }

    pub fn set_sort_schedule(
        &mut self,
        schedule: SurfaceSortSchedule,
    ) -> Result<(), RendererError> {
        if schedule.interval() == 0 {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(target_arch = "wasm32")]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. }) {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. })
            && self.order_backend != SurfaceOrderBackend::Cpu
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
        if self.sort_interval != interval {
            self.sort_interval = interval;
            self.frame_state.force_sort();
        }
        Ok(())
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        self.presenter.set_frame_latency(latency);
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub fn force_sort_refresh(&mut self) {
        self.frame_state.force_sort();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_async_sort_enabled(&mut self, enabled: bool) -> Result<(), RendererError> {
        if self.async_sort_enabled == enabled {
            return Ok(());
        }
        if enabled {
            if self.order_backend != SurfaceOrderBackend::Cpu {
                return Err(RendererError::InvalidConfig);
            }
            self.async_sorter = Some(SurfaceAsyncSorter::new(&self.renderer)?);
        } else {
            self.disable_async_sort();
            return Ok(());
        }
        self.async_sort_enabled = enabled;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn disable_async_sort(&mut self) {
        self.async_sorter = None;
        self.async_sort_enabled = false;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn async_sort_enabled(&self) -> bool {
        self.async_sort_enabled
    }

    pub fn render_frame(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.async_sort_enabled
            && self.order_backend == SurfaceOrderBackend::Cpu
            && self.geometry_path() != GeometryPath::PagedActiveAtlas
        {
            return self.render_frame_async_sort();
        }
        self.render_frame_sync()
    }

    fn render_frame_sync(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let has_order = match self.presented_order_backend {
            SurfaceOrderBackendUsed::Cpu => !self.renderer.current_sorted_indices().is_empty(),
            SurfaceOrderBackendUsed::Gpu => self.gpu_order_initialized,
        };
        let plan = self.frame_state.plan(has_order, self.sort_interval);
        let requested_backend = if !plan.refresh_sort {
            self.presented_order_backend
        } else {
            match self.order_backend {
                SurfaceOrderBackend::Cpu => SurfaceOrderBackendUsed::Cpu,
                SurfaceOrderBackend::Gpu => SurfaceOrderBackendUsed::Gpu,
                SurfaceOrderBackend::Adaptive => self.adaptive_policy.choose_refresh_backend(),
            }
        };
        let mut gpu_failed = false;
        let mut output = if requested_backend == SurfaceOrderBackendUsed::Gpu
            && self.geometry_path() == GeometryPath::SortedIndexDirect
        {
            match self.render_gpu_with_plan(plan) {
                Ok(output) => output,
                Err(_) => {
                    gpu_failed = true;
                    self.frame_state.force_sort();
                    let fallback_plan = self.frame_state.plan(
                        !self.renderer.current_sorted_indices().is_empty(),
                        self.sort_interval,
                    );
                    let mut output = self.render_with_plan(fallback_plan, true)?;
                    output.gpu_sort_fallback = true;
                    output
                }
            }
        } else {
            self.render_with_plan(plan, plan.refresh_sort)?
        };
        // Keep one outer wall clock so a failed GPU attempt plus CPU fallback
        // is measured as the frame the caller actually experienced.
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        output.timings.frame_wall_ms = frame_wall_ms;
        output.stats.frame_ms = frame_wall_ms;
        self.last_stats = output.stats;
        if self.order_backend == SurfaceOrderBackend::Adaptive {
            if gpu_failed {
                self.adaptive_policy.gpu_failed();
            }
            self.adaptive_policy.observe_frame(
                output.order_backend,
                output.timings.frame_wall_ms,
                output.sort_refreshed,
                self.sort_interval,
            );
        }
        output.adaptive_state = if self.order_backend == SurfaceOrderBackend::Adaptive {
            self.adaptive_policy.state()
        } else {
            SurfaceAdaptiveState::Disabled
        };
        if output.sort_refreshed {
            self.applied_order_revision = self.camera_revision;
            self.applied_order_camera = self.camera;
            output.applied_order_revision = self.applied_order_revision;
            output.presented_order_revision_lag = 0;
        }
        Ok(output)
    }

    fn render_gpu_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
    ) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let render_start = timer_now();
        self.presenter
            .render_direct_gpu_order(&self.camera, plan.refresh_sort)?;
        let render_submit_ms = timer_elapsed_ms(render_start);
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        let count = self
            .renderer
            .scene()
            .map(|scene| u32::try_from(scene.len()).unwrap_or(u32::MAX))
            .ok_or(RendererError::SceneNotLoaded)?;
        let stats = FrameStats {
            frame_ms: frame_wall_ms,
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms: 0.0,
            visible_count: count,
            drawn_count: count,
        };
        self.last_stats = stats;
        self.gpu_order_initialized |= plan.refresh_sort;
        self.presented_order_backend = SurfaceOrderBackendUsed::Gpu;
        self.frame_state.finish_frame(plan, false);
        Ok(SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms,
                frame_wall_ms,
            },
            sort_refreshed: plan.refresh_sort,
            order_uploaded: false,
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision: self.camera_revision,
            applied_order_revision: self.applied_order_revision,
            presented_order_revision_lag: u32::try_from(
                self.camera_revision
                    .saturating_sub(self.applied_order_revision),
            )
            .unwrap_or(u32::MAX),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend: SurfaceOrderBackendUsed::Gpu,
            gpu_sort_fallback: false,
            adaptive_state: SurfaceAdaptiveState::Disabled,
        })
    }

    fn render_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
        sort_refreshed: bool,
    ) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let paged = self.geometry_path() == GeometryPath::PagedActiveAtlas;
        let mut stats = if paged {
            FrameStats::zero()
        } else {
            self.renderer
                .build_surface_sorted_indices_with_sort_refresh(&self.camera, plan.refresh_sort)?
        };
        let render_start = timer_now();
        let scene = self.renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
        if paged {
            self.presenter
                .render_sorted_indices(scene, &[], &self.camera, true)?;
            let (visible_count, drawn_count) =
                paged_surface_counts(scene.len(), self.presenter.instance_count());
            stats.visible_count = visible_count;
            stats.drawn_count = drawn_count;
        } else {
            self.presenter.render_sorted_indices(
                scene,
                self.renderer.current_sorted_indices(),
                &self.camera,
                plan.upload_order,
            )?;
        }
        let render_submit_ms = timer_elapsed_ms(render_start);
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        stats.frame_ms = frame_wall_ms;
        self.last_stats = stats;
        self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
        self.frame_state
            .finish_frame(plan, paged || plan.upload_order);
        Ok(SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms,
                frame_wall_ms,
            },
            sort_refreshed: paged || sort_refreshed,
            order_uploaded: paged || plan.upload_order,
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision: self.camera_revision,
            applied_order_revision: self.applied_order_revision,
            presented_order_revision_lag: u32::try_from(
                self.camera_revision
                    .saturating_sub(self.applied_order_revision),
            )
            .unwrap_or(u32::MAX),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_sort_fallback: false,
            adaptive_state: SurfaceAdaptiveState::Disabled,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_frame_async_sort(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let mut completed_timing = None;
        let mut applied_order = false;
        let mut observed_revision_lag = None;
        let mut stale_result_dropped = false;
        let mut completed_revision = None;
        let polled_result = self
            .async_sorter
            .as_mut()
            .ok_or(RendererError::SurfaceWorker)?
            .poll_result();
        if let Some(result) = polled_result {
            let result = result?;
            completed_revision = Some(result.camera_revision);
            let revision_delta = self.camera_revision.saturating_sub(result.camera_revision);
            let revision_lag = u32::try_from(revision_delta).unwrap_or(u32::MAX);
            observed_revision_lag = Some(revision_lag);
            completed_timing = Some((result.preprocess_ms, result.sort_ms));
            if result.camera_revision >= self.applied_order_revision
                && revision_delta <= MAX_ASYNC_SORT_REVISION_LAG
                && async_order_pose_compatible(
                    &result.camera,
                    &self.camera,
                    self.async_sort_translation_limit,
                )
            {
                self.renderer
                    .replace_surface_sorted_indices(result.indices)?;
                self.frame_state.mark_external_order(revision_lag);
                self.applied_order_revision = result.camera_revision;
                self.applied_order_camera = result.camera;
                applied_order = true;
            } else {
                stale_result_dropped = true;
            }
        }

        if self.renderer.current_sorted_indices().is_empty() || self.frame_state.force_sort {
            return self.render_frame_sync();
        }

        let displayed_order_lag = self
            .camera_revision
            .saturating_sub(self.applied_order_revision);
        if displayed_order_lag > MAX_ASYNC_SORT_REVISION_LAG
            || !async_order_pose_compatible(
                &self.applied_order_camera,
                &self.camera,
                self.async_sort_translation_limit,
            )
        {
            let mut output = self.render_frame_sync()?;
            output.async_sort_revision_lag = observed_revision_lag;
            output.stale_async_sort_dropped = stale_result_dropped;
            output.async_sort_completed_revision = completed_revision;
            output.async_sort_result_applied = applied_order;
            output.sync_sort_fallback = true;
            return Ok(output);
        }

        let should_schedule = self.frame_state.camera_dirty
            && self.frame_state.camera_changes_since_sort
                >= async_schedule_threshold(self.sort_interval)
            && !self
                .async_sorter
                .as_ref()
                .is_some_and(SurfaceAsyncSorter::is_in_flight);
        let schedule_camera = self.camera;
        let schedule_revision = self.camera_revision;
        let plan = SurfaceFramePlan {
            refresh_sort: false,
            upload_order: self.frame_state.order_upload_dirty,
        };
        let mut output = self.render_with_plan(plan, applied_order)?;

        if let Some((preprocess_ms, sort_ms)) = completed_timing {
            output.stats.preprocess_ms = preprocess_ms;
            output.stats.sort_ms = sort_ms;
            self.last_stats = output.stats;
        }
        if should_schedule {
            self.async_sorter
                .as_mut()
                .ok_or(RendererError::SurfaceWorker)?
                .start(schedule_camera, schedule_revision);
        }
        output.async_sort_revision_lag = observed_revision_lag;
        output.stale_async_sort_dropped = stale_result_dropped;
        output.async_sort_scheduled = should_schedule;
        output.camera_revision = self.camera_revision;
        output.applied_order_revision = self.applied_order_revision;
        output.presented_order_revision_lag = u32::try_from(
            self.camera_revision
                .saturating_sub(self.applied_order_revision),
        )
        .unwrap_or(u32::MAX);
        output.async_sort_scheduled_revision = should_schedule.then_some(schedule_revision);
        output.async_sort_completed_revision = completed_revision;
        output.async_sort_result_applied = applied_order;
        Ok(output)
    }
}

fn paged_surface_counts(source_count: usize, drawn_count: u32) -> (u32, u32) {
    (u32::try_from(source_count).unwrap_or(u32::MAX), drawn_count)
}

#[cfg(not(target_arch = "wasm32"))]
fn sort_positions_for_camera(
    positions: &[Vec3f],
    camera: Camera,
    camera_revision: u64,
) -> Result<AsyncSortResult, RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    let preprocess_start = std::time::Instant::now();
    let view_rotation = crate::quat_to_mat3(crate::quat_inverse(camera.pose.rotation_xyzw));
    let depth_row = view_rotation[2];
    let camera_position = camera.pose.position;
    let mut depth_keys = Vec::with_capacity(positions.len());
    let mut indices = Vec::with_capacity(positions.len());

    for (index, position) in positions.iter().enumerate() {
        let relative_x = position.x - camera_position.x;
        let relative_y = position.y - camera_position.y;
        let relative_z = position.z - camera_position.z;
        let depth =
            depth_row[0] * relative_x + depth_row[1] * relative_y + depth_row[2] * relative_z;
        if depth >= camera.intrinsics.near_plane && depth <= camera.intrinsics.far_plane {
            indices.push(index as u32);
            depth_keys.push(depth.max(0.0).to_bits());
        }
    }
    let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

    let sort_start = std::time::Instant::now();
    CpuSortBackend::default().sort_values_by_keys(&depth_keys, &mut indices)?;
    let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

    Ok(AsyncSortResult {
        indices,
        preprocess_ms,
        sort_ms,
        camera_revision,
        camera,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn async_order_pose_compatible(
    order_camera: &Camera,
    current_camera: &Camera,
    translation_limit: f32,
) -> bool {
    let dx = current_camera.pose.position.x - order_camera.pose.position.x;
    let dy = current_camera.pose.position.y - order_camera.pose.position.y;
    let dz = current_camera.pose.position.z - order_camera.pose.position.z;
    if dx * dx + dy * dy + dz * dz > translation_limit * translation_limit {
        return false;
    }
    let a = order_camera.pose.rotation_xyzw;
    let b = current_camera.pose.rotation_xyzw;
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3])
        .abs()
        .clamp(0.0, 1.0);
    2.0 * dot.acos() <= MAX_ASYNC_SORT_ROTATION_DELTA_RADIANS
}

#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_GPU_FAILURE_COOLDOWN, ADAPTIVE_INITIAL_PROBE_DELAY, ADAPTIVE_PROBE_SAMPLES,
        ADAPTIVE_REPROBE_INTERVAL, AdaptiveOrderPolicy, AdaptivePhase, AdaptiveTimingEstimate,
        MAX_ASYNC_SORT_REVISION_LAG, SurfaceAdaptiveState, SurfaceFrameState,
        SurfaceOrderBackendUsed, SurfaceSortSchedule, async_order_pose_compatible,
        async_schedule_threshold, paged_surface_counts, try_switch_renderer_geometry_path,
    };
    use crate::{GeometryPath, Renderer};
    use gsplat_core::{Camera, RendererConfig, SceneBuffers, Vec3f};

    #[test]
    fn sort_schedule_exposes_interval_for_sync_and_async_policies() {
        assert_eq!(SurfaceSortSchedule::Interval(2).interval(), 2);
        assert_eq!(
            SurfaceSortSchedule::AsyncLatest { interval: 3 }.interval(),
            3
        );
    }

    #[test]
    fn paged_surface_counts_report_source_total_and_active_drawn() {
        assert_eq!(paged_surface_counts(279_199, 262_144), (279_199, 262_144));
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
        assert_eq!(renderer.world_covariances.as_ref().map(Vec::len), Some(2));

        let result = try_switch_renderer_geometry_path(
            &mut renderer,
            GeometryPath::PagedActiveAtlas,
            |prepared| {
                assert_eq!(prepared.geometry_path(), GeometryPath::PagedActiveAtlas);
                assert!(prepared.world_covariances.is_none());
                assert!(prepared.spatial_pages.is_some());
                Err::<(), _>("injected presenter allocation failure")
            },
        );

        assert_eq!(result, Err("injected presenter allocation failure"));
        assert_eq!(renderer.geometry_path(), GeometryPath::SortedIndexDirect);
        assert_eq!(renderer.world_covariances.as_ref().map(Vec::len), Some(2));
        assert!(renderer.spatial_pages.is_none());
    }

    #[test]
    fn first_frame_forces_sort_and_order_upload() {
        let plan = SurfaceFrameState::default().plan(false, 2);

        assert!(plan.refresh_sort);
        assert!(plan.upload_order);
    }

    #[test]
    fn stationary_frame_reuses_order_without_resorting() {
        let mut state = SurfaceFrameState::default();
        let first = state.plan(false, 2);
        state.finish_frame(first, true);

        let stationary = state.plan(true, 2);
        assert!(!stationary.refresh_sort);
        assert!(!stationary.upload_order);
    }

    #[test]
    fn interval_counts_changed_camera_frames_only() {
        let mut state = SurfaceFrameState::default();
        let first = state.plan(false, 2);
        state.finish_frame(first, true);

        state.mark_camera_changed();
        let first_change = state.plan(true, 2);
        assert!(!first_change.refresh_sort);
        state.finish_frame(first_change, false);

        let stationary = state.plan(true, 2);
        assert!(!stationary.refresh_sort);

        state.mark_camera_changed();
        let second_change = state.plan(true, 2);
        assert!(second_change.refresh_sort);
        assert!(second_change.upload_order);
    }

    #[test]
    fn async_sort_revision_lag_is_explicitly_bounded() {
        assert_eq!(MAX_ASYNC_SORT_REVISION_LAG, 2);
    }

    #[test]
    fn async_sort_pose_envelope_accepts_slow_motion_and_rejects_jumps() {
        let order = Camera::default();
        let mut current = order;
        current.pose.position = Vec3f::new(0.001, 0.0, 0.0);
        current.pose.rotation_xyzw = [0.0, -(0.002_f32 * 0.5).sin(), 0.0, (0.002_f32 * 0.5).cos()];
        assert!(async_order_pose_compatible(&order, &current, 0.01));

        current.pose.position = Vec3f::new(0.02, 0.0, 0.0);
        assert!(!async_order_pose_compatible(&order, &current, 0.01));
        current.pose.position = order.pose.position;
        current.pose.rotation_xyzw = [0.0, -(0.02_f32 * 0.5).sin(), 0.0, (0.02_f32 * 0.5).cos()];
        assert!(!async_order_pose_compatible(&order, &current, 0.01));
    }

    #[test]
    fn async_sort_starts_one_revision_before_interval_boundary() {
        assert_eq!(async_schedule_threshold(1), 1);
        assert_eq!(async_schedule_threshold(2), 1);
        assert_eq!(async_schedule_threshold(3), 2);
    }

    fn feed_cpu_bootstrap(policy: &mut AdaptiveOrderPolicy, frame_ms: f32) {
        policy.reset();
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, frame_ms, true, 1);
        }
        for _ in 0..ADAPTIVE_INITIAL_PROBE_DELAY {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, frame_ms, true, 1);
        }
    }

    #[test]
    fn adaptive_policy_keeps_cpu_when_gpu_probe_loses() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 30.0, true, 1); // compile/warmup sample
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Gpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 20.0, true, 1);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
    }

    #[test]
    fn adaptive_policy_samples_presented_backend_during_interval_reuse() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 30.0, true, 2);
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 20.0, true, 2);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        let cpu_refresh_samples = policy.cpu.refresh.len;
        let cpu_reuse_samples = policy.cpu.reuse.len;
        let gpu_reuse_samples = policy.gpu.reuse.len;
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 21.0, false, 2);

        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        assert_eq!(policy.cpu.refresh.len, cpu_refresh_samples);
        assert_eq!(policy.cpu.reuse.len, cpu_reuse_samples);
        assert_eq!(policy.gpu.reuse.len, gpu_reuse_samples + 1);
    }

    #[test]
    fn adaptive_policy_backs_off_after_losing_gpu_probe() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 30.0, true, 1);
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 20.0, true, 1);
        }

        for _ in 0..ADAPTIVE_REPROBE_INTERVAL {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 10.0, true, 1);
        }
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
    }

    #[test]
    fn adaptive_policy_does_not_misattribute_cpu_reuse_after_cpu_probe() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset();
        policy.cpu.push(12.0, true);
        policy.gpu.push(10.0, true);
        policy.phase = Some(AdaptivePhase::CpuProbe { remaining: 1 });

        policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 12.0, true, 2);
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        let cpu_reuse_samples = policy.cpu.reuse.len;
        let gpu_reuse_samples = policy.gpu.reuse.len;

        policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 12.0, false, 2);
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        assert_eq!(policy.cpu.reuse.len, cpu_reuse_samples + 1);
        assert_eq!(policy.gpu.reuse.len, gpu_reuse_samples);
    }

    #[test]
    fn adaptive_cadence_score_keeps_expensive_refresh_visible_at_long_intervals() {
        let mut cpu = AdaptiveTimingEstimate::default();
        let mut gpu = AdaptiveTimingEstimate::default();
        for _ in 0..4 {
            cpu.push(12.0, true);
            gpu.push(80.0, true);
        }
        for _ in 0..16 {
            cpu.push(10.0, false);
            gpu.push(5.0, false);
        }

        assert!(
            gpu.cadence_score(8).unwrap() > cpu.cadence_score(8).unwrap(),
            "the cheaper GPU reuse frames must not hide its much slower refresh"
        );
    }

    #[test]
    fn adaptive_policy_promotes_gpu_only_after_sustained_win() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 20.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 40.0, true, 1); // ignored warmup
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 15.0, true, 1);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
    }

    #[test]
    fn adaptive_gpu_failure_enters_cpu_cooldown() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset();
        policy.gpu_failed();
        assert_eq!(policy.state(), SurfaceAdaptiveState::Cooldown);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Cpu
        );
    }

    #[test]
    fn adaptive_gpu_failure_retries_after_one_cooldown_period() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset();
        policy.gpu_failed();
        for _ in 0..ADAPTIVE_GPU_FAILURE_COOLDOWN {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 10.0, true, 1);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
    }
}
