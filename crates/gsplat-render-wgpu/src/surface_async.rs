//! Native async CPU sort worker. Not used by the default sync CPU path.

use gsplat_core::Camera;
#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::Vec3f;
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

use crate::{Renderer, RendererError};

pub(crate) const MAX_ASYNC_SORT_REVISION_LAG: u64 = 2;
pub(crate) const MAX_ASYNC_SORT_ROTATION_DELTA_RADIANS: f32 = 0.01;
pub(crate) const MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION: f32 = 0.02;

pub(crate) fn async_schedule_threshold(sort_interval: u32) -> u32 {
    sort_interval.saturating_sub(1).max(1)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct SurfaceAsyncSorter {
    request_tx: SyncSender<Option<(Camera, u64)>>,
    result_rx: Receiver<Result<AsyncSortResult, RendererError>>,
    worker: Option<JoinHandle<()>>,
    in_flight: bool,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct AsyncSortResult {
    pub(crate) indices: Vec<u32>,
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
    pub(crate) camera_revision: u64,
    pub(crate) camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
impl SurfaceAsyncSorter {
    pub(crate) fn new(renderer: &Renderer) -> Result<Self, RendererError> {
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

    pub(crate) fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    pub(crate) fn poll_result(&mut self) -> Option<Result<AsyncSortResult, RendererError>> {
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

    pub(crate) fn start(&mut self, camera: Camera, camera_revision: u64) {
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

    pub(crate) fn drain(&mut self) {
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

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sort_positions_for_camera(
    positions: &[Vec3f],
    camera: Camera,
    camera_revision: u64,
) -> Result<AsyncSortResult, RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    let preprocess_start = std::time::Instant::now();
    let view_rotation =
        crate::math::quat_to_mat3(crate::math::quat_inverse(camera.pose.rotation_xyzw));
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

pub(crate) fn async_order_pose_compatible(
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
