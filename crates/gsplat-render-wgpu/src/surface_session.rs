#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::Vec3f;
use gsplat_core::{Camera, FrameStats};
#[cfg(not(target_arch = "wasm32"))]
use gsplat_sort::CpuSortBackend;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use crate::{Renderer, RendererError, SurfacePresenter};

const DEFAULT_SURFACE_SORT_INTERVAL: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSortSchedule {
    Interval(u32),
    AsyncLatest { interval: u32 },
}

impl SurfaceSortSchedule {
    pub const fn interval(self) -> u32 {
        match self {
            Self::Interval(interval) | Self::AsyncLatest { interval } => interval,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type TimerInstant = std::time::Instant;

#[cfg(target_arch = "wasm32")]
type TimerInstant = f64;

#[cfg(not(target_arch = "wasm32"))]
fn timer_now() -> TimerInstant {
    std::time::Instant::now()
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

#[cfg(not(target_arch = "wasm32"))]
struct SurfaceAsyncSorter {
    positions: Arc<[Vec3f]>,
    in_flight: Option<JoinHandle<Result<AsyncSortResult, RendererError>>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct AsyncSortResult {
    indices: Vec<u32>,
    preprocess_ms: f32,
    sort_ms: f32,
    camera_revision: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl SurfaceAsyncSorter {
    fn new(renderer: &Renderer) -> Result<Self, RendererError> {
        let scene = renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
        Ok(Self {
            positions: Arc::from(scene.positions.clone().into_boxed_slice()),
            in_flight: None,
        })
    }

    fn is_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    fn poll_result(&mut self) -> Option<Result<AsyncSortResult, RendererError>> {
        let handle = self.in_flight.as_ref()?;
        if !handle.is_finished() {
            return None;
        }
        let handle = self.in_flight.take()?;
        Some(handle.join().unwrap_or(Err(RendererError::SurfaceWorker)))
    }

    fn start(&mut self, camera: Camera, camera_revision: u64) {
        if self.in_flight.is_some() {
            return;
        }
        let positions = Arc::clone(&self.positions);
        self.in_flight = Some(thread::spawn(move || {
            sort_positions_for_camera(&positions, camera, camera_revision)
        }));
    }

    fn drain(&mut self) {
        if let Some(handle) = self.in_flight.take() {
            let _ = handle.join();
        }
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
    pub stats: FrameStats,
    pub timings: SurfaceFrameTimings,
    pub sort_refreshed: bool,
    pub order_uploaded: bool,
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

/// Owns the CPU-sort + direct GPU draw lifecycle shared by every Surface client.
///
/// PLY-derived scene attributes stay GPU-resident. Each sort refresh uploads only
/// compact source IDs; the vertex shader fetches and projects the corresponding
/// Gaussian directly for Web, desktop, Android, and iOS.
pub struct SurfaceRenderSession {
    renderer: Renderer,
    presenter: SurfacePresenter,
    camera: Camera,
    sort_interval: u32,
    camera_revision: u64,
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
        Ok(Self {
            renderer,
            presenter,
            camera,
            sort_interval: DEFAULT_SURFACE_SORT_INTERVAL,
            camera_revision: 0,
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
        #[cfg(target_arch = "wasm32")]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. }) {
            return Err(RendererError::InvalidConfig);
        }
        self.set_sort_interval(schedule.interval())?;
        match schedule {
            SurfaceSortSchedule::Interval(_) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.set_async_sort_enabled(false)?;
                Ok(())
            }
            SurfaceSortSchedule::AsyncLatest { .. } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.set_async_sort_enabled(true)?;
                    Ok(())
                }
                #[cfg(target_arch = "wasm32")]
                {
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
            self.async_sorter = Some(SurfaceAsyncSorter::new(&self.renderer)?);
        } else {
            self.async_sorter = None;
        }
        self.async_sort_enabled = enabled;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn async_sort_enabled(&self) -> bool {
        self.async_sort_enabled
    }

    pub fn render_frame(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.async_sort_enabled {
            return self.render_frame_async_sort();
        }
        self.render_frame_sync()
    }

    fn render_frame_sync(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let has_order = !self.renderer.current_sorted_indices().is_empty();
        let plan = self.frame_state.plan(has_order, self.sort_interval);
        self.render_with_plan(plan, plan.refresh_sort)
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
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_frame_async_sort(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let mut completed_timing = None;
        let mut applied_order = false;
        let polled_result = self
            .async_sorter
            .as_mut()
            .ok_or(RendererError::SurfaceWorker)?
            .poll_result();
        if let Some(result) = polled_result {
            let result = result?;
            self.renderer
                .replace_surface_sorted_indices(result.indices)?;
            let revision_delta = self.camera_revision.saturating_sub(result.camera_revision);
            self.frame_state
                .mark_external_order(u32::try_from(revision_delta).unwrap_or(u32::MAX));
            completed_timing = Some((result.preprocess_ms, result.sort_ms));
            applied_order = true;
        }

        if self.renderer.current_sorted_indices().is_empty() || self.frame_state.force_sort {
            return self.render_frame_sync();
        }

        let should_schedule = self.frame_state.camera_dirty
            && self.frame_state.camera_changes_since_sort >= self.sort_interval.max(1)
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
        Ok(output)
    }
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
    })
}

#[cfg(test)]
mod tests {
    use super::{SurfaceFrameState, SurfaceSortSchedule};

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
}
