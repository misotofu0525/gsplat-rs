use gsplat_core::{Camera, FrameStats};

use crate::surface_adaptive::AdaptiveOrderPolicy;
#[cfg(not(target_arch = "wasm32"))]
use crate::surface_async::SurfaceAsyncSorter;
use crate::surface_async::{
    MAX_ASYNC_SORT_REVISION_LAG, MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION,
    async_order_pose_compatible, async_schedule_threshold,
};
use crate::{
    Renderer, RendererError, ResidentStorageProfile, SurfacePresenter, timer_elapsed_ms, timer_now,
};

const DEFAULT_SURFACE_SORT_INTERVAL: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSortSchedule {
    Interval(u32),
    AsyncLatest { interval: u32 },
}

/// Selects where a required resident-scene order refresh is computed. This is
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

impl SurfaceSortSchedule {
    pub const fn interval(self) -> u32 {
        match self {
            Self::Interval(interval) | Self::AsyncLatest { interval } => interval,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFrameTimings {
    /// Retained for API compatibility. Resident rendering performs no CPU geometry expansion.
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

/// Owns the ordering + resident GPU draw lifecycle shared by every Surface client.
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
        #[cfg(not(target_arch = "wasm32"))]
        if backend != SurfaceOrderBackend::Cpu && self.async_sort_enabled {
            return Err(RendererError::InvalidConfig);
        }
        let gpu_prepare_error = if backend == SurfaceOrderBackend::Cpu {
            None
        } else {
            self.presenter.prepare_resident_gpu_order().err()
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

    pub fn storage_profile(&self) -> ResidentStorageProfile {
        self.renderer.storage_profile()
    }

    /// Rebuilds resident GPU buffers for a storage profile. Sample/benchmark
    /// use only; the stable C ABI stays on the default full-f32 layout.
    pub fn set_storage_profile(
        &mut self,
        profile: ResidentStorageProfile,
    ) -> Result<(), RendererError> {
        if self.renderer.storage_profile() == profile {
            return Ok(());
        }
        self.renderer.set_storage_profile(profile);
        self.presenter.rebuild_resident_scene(&self.renderer)?;
        self.gpu_order_initialized = false;
        let gpu_prepare_error = if self.order_backend == SurfaceOrderBackend::Cpu {
            None
        } else {
            self.presenter.prepare_resident_gpu_order().err()
        };
        let gpu_prepare_failed = match (self.order_backend, gpu_prepare_error) {
            (SurfaceOrderBackend::Gpu, Some(error)) => return Err(error.into()),
            (SurfaceOrderBackend::Adaptive, Some(_)) => true,
            _ => false,
        };
        if self.order_backend == SurfaceOrderBackend::Adaptive {
            self.adaptive_policy.reset();
            if gpu_prepare_failed {
                self.adaptive_policy.gpu_failed();
            } else {
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
        if self.async_sort_enabled && self.order_backend == SurfaceOrderBackend::Cpu {
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
        let mut output = if requested_backend == SurfaceOrderBackendUsed::Gpu {
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
            .render_resident_gpu_order(&self.camera, plan.refresh_sort)?;
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
        let mut stats = self
            .renderer
            .build_surface_sorted_indices_with_sort_refresh(&self.camera, plan.refresh_sort)?;
        let render_start = timer_now();
        self.presenter.render_sorted_indices(
            self.renderer.current_sorted_indices(),
            &self.camera,
            plan.upload_order,
        )?;
        let render_submit_ms = timer_elapsed_ms(render_start);
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        stats.frame_ms = frame_wall_ms;
        self.last_stats = stats;
        self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
        self.frame_state.finish_frame(plan, plan.upload_order);
        Ok(SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms,
                frame_wall_ms,
            },
            sort_refreshed,
            order_uploaded: plan.upload_order,
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

#[cfg(test)]
mod tests {
    use super::{SurfaceFrameState, SurfaceSortSchedule};
    use crate::surface_async::{
        MAX_ASYNC_SORT_REVISION_LAG, async_order_pose_compatible, async_schedule_threshold,
    };
    use gsplat_core::{Camera, Vec3f};

    #[test]
    fn sort_schedule_exposes_interval_for_sync_and_async_policies() {
        assert_eq!(SurfaceSortSchedule::Interval(2).interval(), 2);
        assert_eq!(
            SurfaceSortSchedule::AsyncLatest { interval: 3 }.interval(),
            3
        );
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
}
