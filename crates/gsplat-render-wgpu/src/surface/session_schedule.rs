//! Private scheduling state for the shared Surface session.
//!
//! This owner decides when a Direct CPU order is due, owns the native async
//! worker and its bounded queues, and rejects stale completions. It never
//! executes or presents a frame, publishes renderer/cache state, polls GPU
//! telemetry or readback, or emits stats/evidence receipts.

use gsplat_core::Camera;

#[cfg(not(target_arch = "wasm32"))]
use crate::GeometryPath;
#[cfg(not(target_arch = "wasm32"))]
use crate::surface_session::SurfaceOrderBackend;
use crate::{Renderer, RendererError};

pub(crate) const DEFAULT_SURFACE_SORT_INTERVAL: u32 = 1;

/// Maximum number of camera revisions an asynchronously produced order may lag
/// behind the frame that consumes it.
#[cfg(not(target_arch = "wasm32"))]
const MAX_ASYNC_SORT_REVISION_LAG: u64 = 2;
#[cfg(not(target_arch = "wasm32"))]
const MAX_ASYNC_SORT_ROTATION_DELTA_RADIANS: f32 = 0.01;
#[cfg(not(target_arch = "wasm32"))]
const MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION: f32 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceFramePlan {
    pub(crate) refresh_sort: bool,
    pub(crate) upload_order: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScheduleState {
    camera_dirty: bool,
    camera_changed_this_frame: bool,
    force_sort: bool,
    order_upload_dirty: bool,
    camera_changes_since_sort: u32,
}

impl Default for ScheduleState {
    fn default() -> Self {
        Self {
            camera_dirty: true,
            camera_changed_this_frame: true,
            force_sort: true,
            order_upload_dirty: true,
            camera_changes_since_sort: 0,
        }
    }
}

impl ScheduleState {
    fn mark_camera_changed(&mut self) {
        self.camera_dirty = true;
        self.camera_changed_this_frame = true;
        self.camera_changes_since_sort = self.camera_changes_since_sort.saturating_add(1);
    }

    fn force_sort(&mut self) {
        self.force_sort = true;
        self.order_upload_dirty = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn mark_external_order(&mut self, camera_changes_since_sort: u32) {
        self.camera_dirty = camera_changes_since_sort > 0;
        self.camera_changed_this_frame = false;
        self.force_sort = false;
        self.order_upload_dirty = true;
        self.camera_changes_since_sort = camera_changes_since_sort;
    }

    fn plan(self, has_order: bool, sort_interval: u32) -> SurfaceFramePlan {
        let interval = sort_interval.max(1);
        let refresh_sort = self.force_sort
            || !has_order
            || (self.camera_dirty
                && (self.camera_changes_since_sort >= interval || !self.camera_changed_this_frame));
        SurfaceFramePlan {
            refresh_sort,
            upload_order: self.order_upload_dirty || refresh_sort,
        }
    }

    fn finish_presented_frame(&mut self, plan: SurfaceFramePlan, order_uploaded: bool) {
        self.force_sort = false;
        self.camera_changed_this_frame = false;
        if plan.refresh_sort {
            self.camera_dirty = false;
            self.camera_changes_since_sort = 0;
        }
        if order_uploaded {
            self.order_upload_dirty = false;
        }
    }
}

pub(crate) struct SessionSchedule {
    sort_interval: u32,
    schedule_state: ScheduleState,
    applied_order_revision: u64,
    applied_order_camera: Camera,
    #[cfg(not(target_arch = "wasm32"))]
    async_translation_limit: f32,
    #[cfg(not(target_arch = "wasm32"))]
    async_enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    async_worker: Option<NativeCpuOrderWorker>,
}

impl SessionSchedule {
    pub(crate) fn new(renderer: &Renderer, camera: Camera) -> Result<Self, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        let async_translation_limit = scene_translation_limit(renderer)?;
        #[cfg(target_arch = "wasm32")]
        let _ = renderer;

        Ok(Self {
            sort_interval: DEFAULT_SURFACE_SORT_INTERVAL,
            schedule_state: ScheduleState::default(),
            applied_order_revision: 0,
            applied_order_camera: camera,
            #[cfg(not(target_arch = "wasm32"))]
            async_translation_limit,
            #[cfg(not(target_arch = "wasm32"))]
            async_enabled: false,
            #[cfg(not(target_arch = "wasm32"))]
            async_worker: None,
        })
    }

    pub(crate) const fn sort_interval(&self) -> u32 {
        self.sort_interval
    }

    pub(crate) fn set_sort_interval(&mut self, interval: u32) -> bool {
        if self.sort_interval == interval {
            return false;
        }
        self.sort_interval = interval;
        self.force_sort();
        true
    }

    pub(crate) fn mark_camera_changed(&mut self) {
        self.schedule_state.mark_camera_changed();
    }

    pub(crate) fn force_sort(&mut self) {
        self.schedule_state.force_sort();
    }

    pub(crate) fn plan(&self, has_order: bool) -> SurfaceFramePlan {
        self.schedule_state.plan(has_order, self.sort_interval)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn stable_order_plan(&self) -> SurfaceFramePlan {
        SurfaceFramePlan {
            refresh_sort: false,
            upload_order: self.schedule_state.order_upload_dirty,
        }
    }

    pub(crate) fn finish_presented_frame(&mut self, plan: SurfaceFramePlan, order_uploaded: bool) {
        self.schedule_state
            .finish_presented_frame(plan, order_uploaded);
    }

    pub(crate) fn record_applied_order(&mut self, camera: Camera, camera_revision: u64) {
        self.applied_order_revision = camera_revision;
        self.applied_order_camera = camera;
    }

    pub(crate) const fn applied_order_revision(&self) -> u64 {
        self.applied_order_revision
    }

    pub(crate) fn presented_order_revision_lag(&self, camera_revision: u64) -> u32 {
        u32::try_from(camera_revision.saturating_sub(self.applied_order_revision))
            .unwrap_or(u32::MAX)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) const fn configured_async_enabled(&self) -> bool {
        self.async_enabled
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn async_enabled(
        &self,
        geometry_path: GeometryPath,
        order_backend: SurfaceOrderBackend,
    ) -> bool {
        self.async_enabled && async_sort_supported(geometry_path, order_backend)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_async_enabled(
        &mut self,
        renderer: &Renderer,
        enabled: bool,
    ) -> Result<bool, RendererError> {
        if self.async_enabled == enabled {
            return Ok(false);
        }
        if enabled {
            self.async_worker = Some(NativeCpuOrderWorker::new(renderer_positions(renderer)?)?);
            self.async_enabled = true;
        } else {
            self.disable_async();
        }
        Ok(true)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn disable_async(&mut self) {
        self.async_worker = None;
        self.async_enabled = false;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn poll_async_order(
        &mut self,
        current_camera: &Camera,
        current_revision: u64,
    ) -> Result<AsyncOrderPoll, RendererError> {
        let Some(result) = self
            .async_worker
            .as_mut()
            .ok_or(RendererError::SurfaceWorker)?
            .poll_result()
        else {
            return Ok(AsyncOrderPoll::default());
        };
        let result = result?;
        let revision_delta = current_revision.saturating_sub(result.camera_revision);
        let revision_lag = u32::try_from(revision_delta).unwrap_or(u32::MAX);
        let mut poll = AsyncOrderPoll {
            completed_timing: Some((result.timings.preprocess_ms, result.timings.sort_ms)),
            observed_revision_lag: Some(revision_lag),
            completed_revision: Some(result.camera_revision),
            ..AsyncOrderPoll::default()
        };
        if async_order_result_is_usable(
            result.camera_revision,
            self.applied_order_revision,
            revision_delta,
            &result.camera,
            current_camera,
            self.async_translation_limit,
        ) {
            poll.candidate = Some(AsyncOrderCandidate {
                ordered_ids: result.ordered_ids,
                camera_revision: result.camera_revision,
                camera: result.camera,
                revision_lag,
            });
        } else {
            poll.stale_result_dropped = true;
            self.async_worker
                .as_mut()
                .expect("enabled async worker")
                .recycle(result.ordered_ids);
        }
        Ok(poll)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn recycle_async_order(&mut self, ordered_ids: Vec<u32>) {
        self.async_worker
            .as_mut()
            .expect("enabled async worker")
            .recycle(ordered_ids);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn accept_async_order(
        &mut self,
        camera: Camera,
        camera_revision: u64,
        revision_lag: u32,
    ) {
        self.schedule_state.mark_external_order(revision_lag);
        self.applied_order_revision = camera_revision;
        self.applied_order_camera = camera;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn requires_initial_sync(&self, has_order: bool) -> bool {
        !has_order || self.schedule_state.force_sort
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn requires_stale_order_fallback(
        &self,
        current_camera: &Camera,
        current_revision: u64,
    ) -> bool {
        current_revision.saturating_sub(self.applied_order_revision) > MAX_ASYNC_SORT_REVISION_LAG
            || !async_order_pose_compatible(
                &self.applied_order_camera,
                current_camera,
                self.async_translation_limit,
            )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn should_schedule_async(&self) -> bool {
        self.schedule_state.camera_dirty
            && self.schedule_state.camera_changes_since_sort
                >= async_schedule_threshold(self.sort_interval)
            && !self
                .async_worker
                .as_ref()
                .is_some_and(NativeCpuOrderWorker::is_in_flight)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_async_order(&mut self, camera: Camera, camera_revision: u64) {
        self.async_worker
            .as_mut()
            .expect("enabled async worker")
            .start(camera, camera_revision);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct AsyncOrderPoll {
    pub(crate) candidate: Option<AsyncOrderCandidate>,
    pub(crate) completed_timing: Option<(f32, f32)>,
    pub(crate) observed_revision_lag: Option<u32>,
    pub(crate) stale_result_dropped: bool,
    pub(crate) completed_revision: Option<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct AsyncOrderCandidate {
    pub(crate) ordered_ids: Vec<u32>,
    pub(crate) camera_revision: u64,
    pub(crate) camera: Camera,
    pub(crate) revision_lag: u32,
}

#[cfg(not(target_arch = "wasm32"))]
fn renderer_positions(
    renderer: &Renderer,
) -> Result<std::sync::Arc<[gsplat_core::Vec3f]>, RendererError> {
    if let Some(scene) = renderer.resident_scene() {
        Ok(std::sync::Arc::clone(&scene.positions))
    } else {
        let scene = renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
        Ok(std::sync::Arc::from(
            scene.positions.clone().into_boxed_slice(),
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn scene_translation_limit(renderer: &Renderer) -> Result<f32, RendererError> {
    let positions = renderer.positions().ok_or(RendererError::SceneNotLoaded)?;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for position in positions {
        min[0] = min[0].min(position.x);
        min[1] = min[1].min(position.y);
        min[2] = min[2].min(position.z);
        max[0] = max[0].max(position.x);
        max[1] = max[1].max(position.y);
        max[2] = max[2].max(position.z);
    }
    let diagonal =
        ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2)).sqrt();
    Ok((diagonal * MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION).max(1e-4))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn async_sort_supported(
    geometry_path: GeometryPath,
    order_backend: SurfaceOrderBackend,
) -> bool {
    geometry_path == GeometryPath::SortedIndexDirect && order_backend == SurfaceOrderBackend::Cpu
}

#[cfg(not(target_arch = "wasm32"))]
fn async_schedule_threshold(sort_interval: u32) -> u32 {
    sort_interval.saturating_sub(1).max(1)
}

#[cfg(not(target_arch = "wasm32"))]
fn async_order_result_is_usable(
    result_revision: u64,
    applied_revision: u64,
    revision_delta: u64,
    result_camera: &Camera,
    current_camera: &Camera,
    translation_limit: f32,
) -> bool {
    result_revision >= applied_revision
        && revision_delta <= MAX_ASYNC_SORT_REVISION_LAG
        && async_order_pose_compatible(result_camera, current_camera, translation_limit)
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

#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::cpu_order::{CpuOrderEngine, CpuOrderTimings};
#[cfg(not(target_arch = "wasm32"))]
use crate::data::OwnedCpuOrderInput;

#[cfg(not(target_arch = "wasm32"))]
struct NativeCpuOrderWorker {
    request_tx: SyncSender<Option<NativeCpuOrderRequest>>,
    result_rx: Receiver<NativeCpuOrderCompletion>,
    worker: Option<JoinHandle<()>>,
    recycled_ids: Vec<u32>,
    in_flight: bool,
    #[cfg(test)]
    join_started: Option<Arc<std::sync::Barrier>>,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[derive(Clone)]
struct NativeCpuOrderWorkerTestControl {
    request_started: Arc<std::sync::Barrier>,
    release_request: Arc<std::sync::Barrier>,
    join_started: Arc<std::sync::Barrier>,
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeCpuOrderRequest {
    camera: Camera,
    camera_revision: u64,
    recycled_ids: Vec<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeCpuOrderCompletion {
    ordered_ids: Vec<u32>,
    result: Result<NativeCpuOrderMetadata, RendererError>,
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeCpuOrderMetadata {
    timings: CpuOrderTimings,
    camera_revision: u64,
    camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeCpuOrderResult {
    ordered_ids: Vec<u32>,
    timings: CpuOrderTimings,
    camera_revision: u64,
    camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeCpuOrderWorker {
    fn new(positions: Arc<[gsplat_core::Vec3f]>) -> Result<Self, RendererError> {
        Self::new_inner(
            positions,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    fn new_for_in_flight_teardown_test(
        positions: Arc<[gsplat_core::Vec3f]>,
        control: NativeCpuOrderWorkerTestControl,
    ) -> Result<Self, RendererError> {
        Self::new_inner(positions, Some(control))
    }

    fn new_inner(
        positions: Arc<[gsplat_core::Vec3f]>,
        #[cfg(test)] test_control: Option<NativeCpuOrderWorkerTestControl>,
    ) -> Result<Self, RendererError> {
        let engine = CpuOrderEngine::try_with_capacity(positions.len())
            .map_err(|_| RendererError::SurfaceWorker)?;
        let mut recycled_ids = Vec::new();
        recycled_ids
            .try_reserve_exact(positions.len())
            .map_err(|_| RendererError::SurfaceWorker)?;
        let (request_tx, request_rx) = sync_channel::<Option<NativeCpuOrderRequest>>(1);
        let (result_tx, result_rx) = sync_channel(1);
        #[cfg(test)]
        let worker_test_control = test_control.clone();
        let worker = thread::spawn(move || {
            let mut positions = positions;
            let mut engine = engine;
            while let Ok(request) = request_rx.recv() {
                let Some(request) = request else {
                    break;
                };
                #[cfg(test)]
                if let Some(control) = worker_test_control.as_ref() {
                    control.request_started.wait();
                    control.release_request.wait();
                }
                let input = OwnedCpuOrderInput::new(positions, request.camera);
                let mut ordered_ids = request.recycled_ids;
                let camera = input.camera();
                let result = engine
                    .order_positions(input.position_view(), &camera, true, &mut ordered_ids)
                    .map(|timings| NativeCpuOrderMetadata {
                        timings,
                        camera_revision: request.camera_revision,
                        camera,
                    });
                positions = input.into_positions();
                if result_tx
                    .send(NativeCpuOrderCompletion {
                        ordered_ids,
                        result,
                    })
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
            recycled_ids,
            in_flight: false,
            #[cfg(test)]
            join_started: test_control.map(|control| control.join_started),
        })
    }

    const fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    fn poll_result(&mut self) -> Option<Result<NativeCpuOrderResult, RendererError>> {
        if !self.in_flight {
            return None;
        }
        match self.result_rx.try_recv() {
            Ok(completion) => {
                self.in_flight = false;
                match completion.result {
                    Ok(metadata) => Some(Ok(NativeCpuOrderResult {
                        ordered_ids: completion.ordered_ids,
                        timings: metadata.timings,
                        camera_revision: metadata.camera_revision,
                        camera: metadata.camera,
                    })),
                    Err(error) => {
                        self.recycled_ids = completion.ordered_ids;
                        Some(Err(error))
                    }
                }
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
        let request = NativeCpuOrderRequest {
            camera,
            camera_revision,
            recycled_ids: std::mem::take(&mut self.recycled_ids),
        };
        match self.request_tx.try_send(Some(request)) {
            Ok(()) => self.in_flight = true,
            Err(TrySendError::Full(Some(request)) | TrySendError::Disconnected(Some(request))) => {
                self.recycled_ids = request.recycled_ids;
            }
            Err(TrySendError::Full(None) | TrySendError::Disconnected(None)) => {}
        }
    }

    fn recycle(&mut self, ordered_ids: Vec<u32>) {
        debug_assert!(!self.in_flight);
        self.recycled_ids = ordered_ids;
    }

    fn drain(&mut self) {
        let _ = self.request_tx.send(None);
        if let Some(handle) = self.worker.take() {
            #[cfg(test)]
            if let Some(join_started) = self.join_started.take() {
                join_started.wait();
            }
            let _ = handle.join();
        }
        self.in_flight = false;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for NativeCpuOrderWorker {
    fn drop(&mut self) {
        self.drain();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use gsplat_core::{RendererConfig, SceneBuffers, Vec3f};

    #[test]
    fn first_frame_forces_sort_and_order_upload() {
        let plan = ScheduleState::default().plan(false, 2);
        assert!(plan.refresh_sort);
        assert!(plan.upload_order);
    }

    #[test]
    fn stationary_frame_reuses_order_without_resorting() {
        let mut state = ScheduleState::default();
        let first = state.plan(false, 2);
        state.finish_presented_frame(first, true);
        let stationary = state.plan(true, 2);
        assert!(!stationary.refresh_sort);
        assert!(!stationary.upload_order);
    }

    #[test]
    fn deferred_camera_change_catches_up_on_the_next_stationary_frame() {
        let mut state = ScheduleState::default();
        let first = state.plan(false, 2);
        state.finish_presented_frame(first, true);
        state.mark_camera_changed();
        let first_change = state.plan(true, 2);
        assert!(!first_change.refresh_sort);
        state.finish_presented_frame(first_change, false);
        let stationary = state.plan(true, 2);
        assert!(stationary.refresh_sort);
        assert!(stationary.upload_order);
        state.finish_presented_frame(stationary, true);
        let caught_up = state.plan(true, 2);
        assert!(!caught_up.refresh_sort);
        assert!(!caught_up.upload_order);
    }

    #[test]
    fn continuous_camera_changes_refresh_at_the_requested_interval() {
        let mut state = ScheduleState::default();
        let first = state.plan(false, 2);
        state.finish_presented_frame(first, true);
        state.mark_camera_changed();
        let first_change = state.plan(true, 2);
        assert!(!first_change.refresh_sort);
        state.finish_presented_frame(first_change, false);
        state.mark_camera_changed();
        let second_change = state.plan(true, 2);
        assert!(second_change.refresh_sort);
        assert!(second_change.upload_order);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn async_result_requires_monotonic_revision_bounded_lag_and_pose() {
        let order = Camera::default();
        let mut current = order;
        current.pose.position = Vec3f::new(0.001, 0.0, 0.0);
        current.pose.rotation_xyzw = [0.0, -(0.002_f32 * 0.5).sin(), 0.0, (0.002_f32 * 0.5).cos()];
        assert!(async_order_result_is_usable(
            5, 4, 2, &order, &current, 0.01
        ));
        assert!(!async_order_result_is_usable(
            3, 4, 2, &order, &current, 0.01
        ));
        assert!(!async_order_result_is_usable(
            5, 4, 3, &order, &current, 0.01
        ));
        current.pose.position = Vec3f::new(0.02, 0.0, 0.0);
        assert!(!async_order_result_is_usable(
            5, 4, 2, &order, &current, 0.01
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn async_sort_starts_one_revision_before_interval_boundary() {
        assert_eq!(async_schedule_threshold(1), 1);
        assert_eq!(async_schedule_threshold(2), 1);
        assert_eq!(async_schedule_threshold(3), 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wait_for_async_result(
        sorter: &mut NativeCpuOrderWorker,
    ) -> Result<NativeCpuOrderResult, RendererError> {
        for _ in 0..1_000_000 {
            if let Some(result) = sorter.poll_result() {
                return result;
            }
            std::thread::yield_now();
        }
        panic!("async CPU order worker did not complete")
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn async_worker_matches_sync_recycles_two_buffers_and_fails_without_publication() {
        let count = 257;
        let scene = SceneBuffers {
            positions: (0..count)
                .map(|index| {
                    let depth = match index % 5 {
                        0 | 1 => 2.0,
                        2 => 1.0,
                        3 => 3.0,
                        _ => 4.0,
                    };
                    Vec3f::new(index as f32 * 0.001, 0.0, depth)
                })
                .collect(),
            opacity: vec![1.0; count],
            scale_xyz: vec![[0.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0; 3]; count],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = 1.0;
        camera.intrinsics.far_plane = 3.0;
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(scene).unwrap();
        renderer
            .build_surface_sorted_indices_with_sort_refresh(&camera, true)
            .expect("sync order");
        let authoritative = renderer.current_sorted_indices().to_vec();

        let mut sorter = NativeCpuOrderWorker::new(renderer_positions(&renderer).unwrap())
            .expect("async worker");
        let mut pointers = Vec::new();
        for revision in 1..=3 {
            sorter.start(camera, revision);
            let result = wait_for_async_result(&mut sorter).expect("async order");
            assert_eq!(result.ordered_ids, authoritative);
            assert_eq!(result.camera_revision, revision);
            pointers.push((
                result.ordered_ids.as_ptr() as usize,
                result.ordered_ids.capacity(),
            ));
            sorter.recycle(result.ordered_ids);
        }
        assert_ne!(pointers[0].0, pointers[1].0);
        assert_eq!(pointers[0], pointers[2]);

        let mut invalid = camera;
        invalid.intrinsics.near_plane = 4.0;
        invalid.intrinsics.far_plane = 1.0;
        sorter.start(invalid, 4);
        assert!(wait_for_async_result(&mut sorter).is_err());
        assert_eq!(renderer.current_sorted_indices(), authoritative);

        sorter.start(camera, 5);
        let recovered = wait_for_async_result(&mut sorter).expect("recovered async order");
        assert_eq!(recovered.ordered_ids, authoritative);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn in_flight_disable_joins_old_worker_before_enable_creates_replacement() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 2.0), Vec3f::new(0.0, 0.0, 3.0)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[0.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let camera = Camera::default();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(scene).unwrap();
        let mut schedule = SessionSchedule::new(&renderer, camera).unwrap();

        let old_positions = renderer_positions(&renderer).unwrap();
        let old_positions_weak = Arc::downgrade(&old_positions);
        let request_started = Arc::new(std::sync::Barrier::new(2));
        let release_request = Arc::new(std::sync::Barrier::new(2));
        let join_started = Arc::new(std::sync::Barrier::new(2));
        schedule.async_worker = Some(
            NativeCpuOrderWorker::new_for_in_flight_teardown_test(
                old_positions,
                NativeCpuOrderWorkerTestControl {
                    request_started: Arc::clone(&request_started),
                    release_request: Arc::clone(&release_request),
                    join_started: Arc::clone(&join_started),
                },
            )
            .expect("old async worker"),
        );
        schedule.async_enabled = true;
        schedule.start_async_order(camera, 1);
        request_started.wait();

        let (replacement_tx, replacement_rx) = sync_channel(1);
        let transition = thread::spawn(move || {
            schedule.disable_async();
            let old_worker_exited = old_positions_weak.upgrade().is_none();
            schedule
                .set_async_enabled(&renderer, true)
                .expect("replacement async worker");
            replacement_tx
                .send((old_worker_exited, schedule.configured_async_enabled()))
                .expect("replacement observation");
            schedule
        });

        join_started.wait();
        assert!(matches!(
            replacement_rx.try_recv(),
            Err(TryRecvError::Empty)
        ));
        release_request.wait();

        let (old_worker_exited, replacement_enabled) = replacement_rx
            .recv()
            .expect("replacement created after old worker exit");
        assert!(old_worker_exited);
        assert!(replacement_enabled);
        let schedule = transition.join().expect("disable-enable transition");
        drop(schedule);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stale_worker_result_is_recycled_and_cannot_replace_newer_schedule_identity() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 2.0), Vec3f::new(0.0, 0.0, 3.0)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[0.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let camera = Camera::default();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(scene).unwrap();
        let mut schedule = SessionSchedule::new(&renderer, camera).unwrap();
        schedule.set_async_enabled(&renderer, true).unwrap();
        schedule.record_applied_order(camera, 5);

        schedule.start_async_order(camera, 4);
        let stale = loop {
            let poll = schedule.poll_async_order(&camera, 6).unwrap();
            if poll.completed_revision.is_some() {
                break poll;
            }
            std::thread::yield_now();
        };
        assert!(stale.candidate.is_none());
        assert!(stale.stale_result_dropped);
        assert_eq!(schedule.applied_order_revision(), 5);

        schedule.start_async_order(camera, 6);
        let fresh = loop {
            let poll = schedule.poll_async_order(&camera, 6).unwrap();
            if poll.completed_revision.is_some() {
                break poll;
            }
            std::thread::yield_now();
        };
        let candidate = fresh.candidate.expect("fresh result remains a candidate");
        assert!(!fresh.stale_result_dropped);
        schedule.recycle_async_order(candidate.ordered_ids);
        assert_eq!(schedule.applied_order_revision(), 5);
    }
}
