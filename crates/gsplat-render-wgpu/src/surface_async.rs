//! Native async CPU sort worker. Not used by the default sync CPU path.

use gsplat_core::Camera;
#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::SceneBuffers;
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
enum AsyncSortRequest {
    Sort {
        camera: Camera,
        camera_revision: u64,
        /// Output buffer for the sorted source IDs; its capacity is reused.
        indices: Vec<u32>,
    },
    Stop,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct SurfaceAsyncSorter {
    request_tx: SyncSender<AsyncSortRequest>,
    result_rx: Receiver<Result<AsyncSortResult, RendererError>>,
    worker: Option<JoinHandle<()>>,
    in_flight: bool,
    /// Index buffer the session no longer displays, handed to the next
    /// request. Two buffers ping-pong between the threads in steady state.
    recycled_indices: Option<Vec<u32>>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct AsyncSortResult {
    pub(crate) indices: Vec<u32>,
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
    pub(crate) camera_revision: u64,
    pub(crate) camera: Camera,
}

/// Buffers the worker keeps between requests: depth keys for the visible set
/// and the radix backend's packed/scratch/histogram storage.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct AsyncSortWorkspace {
    depth_keys: Vec<u32>,
    backend: CpuSortBackend,
}

#[cfg(not(target_arch = "wasm32"))]
impl AsyncSortWorkspace {
    #[cfg(test)]
    pub(crate) fn depth_key_capacity(&self) -> usize {
        self.depth_keys.capacity()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SurfaceAsyncSorter {
    pub(crate) fn new(renderer: &Renderer) -> Result<Self, RendererError> {
        let scene = Arc::clone(renderer.scene_arc().ok_or(RendererError::SceneNotLoaded)?);
        let (request_tx, request_rx) = sync_channel::<AsyncSortRequest>(1);
        let (result_tx, result_rx) = sync_channel(1);
        let worker = thread::spawn(move || {
            let mut workspace = AsyncSortWorkspace::default();
            while let Ok(request) = request_rx.recv() {
                let AsyncSortRequest::Sort {
                    camera,
                    camera_revision,
                    indices,
                } = request
                else {
                    break;
                };
                if result_tx
                    .send(sort_scene_for_camera(
                        &scene,
                        camera,
                        camera_revision,
                        &mut workspace,
                        indices,
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
            recycled_indices: None,
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

    /// Returns an index buffer the session has finished with so the next
    /// request writes into it instead of allocating.
    pub(crate) fn recycle(&mut self, indices: Vec<u32>) {
        if self
            .recycled_indices
            .as_ref()
            .is_none_or(|kept| kept.capacity() < indices.capacity())
        {
            self.recycled_indices = Some(indices);
        }
    }

    pub(crate) fn start(&mut self, camera: Camera, camera_revision: u64) {
        if self.in_flight {
            return;
        }
        let indices = self.recycled_indices.take().unwrap_or_default();
        match self.request_tx.try_send(AsyncSortRequest::Sort {
            camera,
            camera_revision,
            indices,
        }) {
            Ok(()) => self.in_flight = true,
            Err(std::sync::mpsc::TrySendError::Full(AsyncSortRequest::Sort {
                indices, ..
            }))
            | Err(std::sync::mpsc::TrySendError::Disconnected(AsyncSortRequest::Sort {
                indices,
                ..
            })) => self.recycled_indices = Some(indices),
            Err(_) => {}
        }
    }

    pub(crate) fn drain(&mut self) {
        let _ = self.request_tx.send(AsyncSortRequest::Stop);
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

/// Runs the shared CPU visibility pass for `scene`, writing depth keys into
/// `workspace`, then sorts the visible source IDs back-to-front into
/// `indices`. A warm workspace and a recycled `indices` buffer allocate
/// nothing.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sort_scene_for_camera(
    scene: &SceneBuffers,
    camera: Camera,
    camera_revision: u64,
    workspace: &mut AsyncSortWorkspace,
    mut indices: Vec<u32>,
) -> Result<AsyncSortResult, RendererError> {
    let preprocess_start = std::time::Instant::now();
    crate::preprocess::preprocess_visible_into(
        scene,
        &camera,
        &mut workspace.depth_keys,
        &mut indices,
    )?;
    let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

    let sort_start = std::time::Instant::now();
    workspace
        .backend
        .sort_values_by_keys(&workspace.depth_keys, &mut indices)?;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{AsyncSortWorkspace, SurfaceAsyncSorter, sort_scene_for_camera};
    use crate::Renderer;
    use gsplat_core::{Camera, RenderMode, SceneBuffers, Vec3f};

    fn synthetic_scene(count: usize) -> SceneBuffers {
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let positions = (0..count)
            .map(|_| Vec3f::new(next() * 4.0 - 2.0, next() * 4.0 - 2.0, next() * 40.0 + 0.5))
            .collect();
        SceneBuffers {
            positions,
            opacity: vec![1.0; count],
            scale_xyz: vec![[0.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.5; 3]; count],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    fn scene_for_bench() -> (SceneBuffers, &'static str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/external/wakufactory_kitune/kitune1.ply");
        if path.is_file()
            && let Ok(loaded) = gsplat_io_ply::load_ply(&path)
        {
            return (loaded.scene, "kitsune");
        }
        (synthetic_scene(200_000), "synthetic-200k")
    }

    #[test]
    fn warm_workspace_and_recycled_indices_allocate_nothing() {
        let scene = synthetic_scene(50_000);
        let camera = Camera::default();
        let mut workspace = AsyncSortWorkspace::default();

        let first = sort_scene_for_camera(&scene, camera, 1, &mut workspace, Vec::new()).unwrap();
        let visible = first.indices.len();
        assert!(visible > 0);
        let key_capacity = workspace.depth_key_capacity();
        let indices_ptr = first.indices.as_ptr();
        let indices_capacity = first.indices.capacity();

        let second =
            sort_scene_for_camera(&scene, camera, 2, &mut workspace, first.indices).unwrap();
        assert_eq!(second.indices.len(), visible);
        assert_eq!(second.indices.as_ptr(), indices_ptr);
        assert_eq!(second.indices.capacity(), indices_capacity);
        assert_eq!(workspace.depth_key_capacity(), key_capacity);
    }

    #[test]
    fn worker_shares_the_scene_and_reuses_the_recycled_index_buffer() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(synthetic_scene(10_000)).unwrap();
        let scene_ptr = renderer.scene().map(|scene| scene.positions.as_ptr());
        let mut sorter = SurfaceAsyncSorter::new(&renderer).unwrap();
        assert_eq!(
            renderer.scene_arc().map(std::sync::Arc::strong_count),
            Some(2),
            "the worker must share the renderer's scene instead of copying it"
        );
        assert_eq!(
            renderer.scene().map(|scene| scene.positions.as_ptr()),
            scene_ptr
        );
        let camera = Camera::default();

        let wait_result = |sorter: &mut SurfaceAsyncSorter| loop {
            if let Some(result) = sorter.poll_result() {
                return result.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        };

        sorter.start(camera, 1);
        let first = wait_result(&mut sorter);
        let ptr = first.indices.as_ptr();
        sorter.recycle(first.indices);

        sorter.start(camera, 2);
        let second = wait_result(&mut sorter);
        assert_eq!(second.camera_revision, 2);
        assert_eq!(
            second.indices.as_ptr(),
            ptr,
            "the worker must sort into the buffer the session returned"
        );
    }

    /// Before/after in one command: a fresh workspace per request (the
    /// previous per-sort allocation pattern) versus one warm workspace with a
    /// recycled index buffer.
    #[test]
    fn workspace_reuse_microbench() {
        let (scene, label) = scene_for_bench();
        let camera = Camera::default();
        const ITERS: u32 = 20;

        let fresh_started = std::time::Instant::now();
        let mut visible = 0;
        for revision in 0..ITERS {
            let mut workspace = AsyncSortWorkspace::default();
            let result = sort_scene_for_camera(
                &scene,
                camera,
                u64::from(revision),
                &mut workspace,
                Vec::new(),
            )
            .unwrap();
            visible = result.indices.len();
        }
        let fresh_ms = fresh_started.elapsed().as_secs_f64() * 1000.0 / f64::from(ITERS);

        let mut workspace = AsyncSortWorkspace::default();
        let mut indices = Vec::new();
        let warm = sort_scene_for_camera(&scene, camera, 0, &mut workspace, indices).unwrap();
        indices = warm.indices;
        let reuse_started = std::time::Instant::now();
        for revision in 1..=ITERS {
            let result =
                sort_scene_for_camera(&scene, camera, u64::from(revision), &mut workspace, indices)
                    .unwrap();
            indices = result.indices;
        }
        let reuse_ms = reuse_started.elapsed().as_secs_f64() * 1000.0 / f64::from(ITERS);
        eprintln!(
            "async_sort_workspace scene={label} splats={} visible={visible} iters={ITERS} fresh_workspace_ms={fresh_ms:.3} reused_workspace_ms={reuse_ms:.3}",
            scene.len()
        );
        assert!(fresh_ms.is_finite() && reuse_ms.is_finite());
    }
}
