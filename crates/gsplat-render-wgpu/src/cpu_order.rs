use gsplat_core::{Camera, SceneBuffers, Vec3f};
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
};

use crate::RendererError;
use crate::cpu::preprocess;
#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::cpu::preprocess::PreprocessChunkScratch;
use crate::cpu::workspace::{CpuOrderWorkspace, WorkspaceAllocationError};
use crate::data::CpuPositionView;
#[cfg(not(target_arch = "wasm32"))]
use crate::data::OwnedCpuOrderInput;

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use crate::cpu::preprocess::PARALLEL_PREPROCESS_THRESHOLD;

#[derive(Debug)]
pub(crate) struct CpuOrderAllocationError {
    resource: &'static str,
}

impl CpuOrderAllocationError {
    pub(crate) const fn resource(&self) -> &'static str {
        self.resource
    }
}

impl From<WorkspaceAllocationError> for CpuOrderAllocationError {
    fn from(error: WorkspaceAllocationError) -> Self {
        Self {
            resource: error.resource,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CpuOrderTimings {
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
}

/// One lane-local exact CPU ordering engine.
///
/// The engine owns preprocess buffers and the reusable `gsplat-sort` radix
/// backend. The caller owns the authoritative ID buffer; successful execution
/// swaps a completed candidate into that buffer as the final step. This keeps
/// sync Surface, native async, offscreen/legacy and shadow CPU PostSort lanes
/// independent without a global mutex.
#[derive(Default)]
pub(crate) struct CpuOrderEngine {
    workspace: CpuOrderWorkspace,
}

impl CpuOrderEngine {
    pub(crate) fn try_with_capacity(source_count: usize) -> Result<Self, CpuOrderAllocationError> {
        Ok(Self {
            workspace: CpuOrderWorkspace::try_with_capacity(source_count)?,
        })
    }

    pub(crate) fn order_positions(
        &mut self,
        positions: CpuPositionView<'_>,
        camera: &Camera,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
    ) -> Result<CpuOrderTimings, RendererError> {
        let timings =
            self.workspace
                .order_positions(positions, camera, stable_full32, authoritative_ids)?;
        Ok(CpuOrderTimings {
            preprocess_ms: timings.preprocess_ms,
            sort_ms: timings.sort_ms,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn order_paged(
        &mut self,
        scene: &SceneBuffers,
        entries: &[(u32, u32)],
        camera: &Camera,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
    ) -> Result<CpuOrderTimings, RendererError> {
        let timings =
            self.workspace
                .order_paged(scene, entries, camera, stable_full32, authoritative_ids)?;
        Ok(CpuOrderTimings {
            preprocess_ms: timings.preprocess_ms,
            sort_ms: timings.sort_ms,
        })
    }

    #[cfg(test)]
    pub(crate) fn buffer_state(&self) -> ((usize, usize), (usize, usize)) {
        self.workspace.buffer_state()
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Persistent native worker for one async Surface lane.
///
/// Its ID storage is a bounded two-buffer cycle: the engine workspace owns one
/// candidate vector, while one recyclable vector moves through the request,
/// result and renderer handoff. The renderer's synchronous engine and current
/// authoritative order are a separate lane-local pair.
pub(crate) struct NativeCpuOrderWorker {
    request_tx: SyncSender<Option<NativeCpuOrderRequest>>,
    result_rx: Receiver<NativeCpuOrderCompletion>,
    worker: Option<JoinHandle<()>>,
    recycled_ids: Vec<u32>,
    in_flight: bool,
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
pub(crate) struct NativeCpuOrderResult {
    pub(crate) ordered_ids: Vec<u32>,
    pub(crate) timings: CpuOrderTimings,
    pub(crate) camera_revision: u64,
    pub(crate) camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeCpuOrderWorker {
    pub(crate) fn new(positions: Arc<[Vec3f]>) -> Result<Self, RendererError> {
        let engine = CpuOrderEngine::try_with_capacity(positions.len())
            .map_err(|_| RendererError::SurfaceWorker)?;
        let mut recycled_ids = Vec::new();
        recycled_ids
            .try_reserve_exact(positions.len())
            .map_err(|_| RendererError::SurfaceWorker)?;
        let (request_tx, request_rx) = sync_channel::<Option<NativeCpuOrderRequest>>(1);
        let (result_tx, result_rx) = sync_channel(1);
        let worker = thread::spawn(move || {
            let mut positions = positions;
            let mut engine = engine;
            while let Ok(request) = request_rx.recv() {
                let Some(request) = request else {
                    break;
                };
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
        })
    }

    pub(crate) const fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    pub(crate) fn poll_result(&mut self) -> Option<Result<NativeCpuOrderResult, RendererError>> {
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

    pub(crate) fn start(&mut self, camera: Camera, camera_revision: u64) {
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

    pub(crate) fn recycle(&mut self, ordered_ids: Vec<u32>) {
        debug_assert!(!self.in_flight);
        self.recycled_ids = ordered_ids;
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
impl Drop for NativeCpuOrderWorker {
    fn drop(&mut self) {
        self.drain();
    }
}

pub(crate) fn is_visible(depth_z: f32, camera: &Camera) -> bool {
    depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane
}

#[cfg(test)]
pub(crate) fn depth_to_key(depth_z: f32) -> u32 {
    depth_z.max(0.0).to_bits()
}

/// Scalar authority retained for focused parity tests and narrow callers that
/// need visibility preprocessing without an ordering workspace.
pub(crate) fn preprocess_positions_visible_into(
    positions: &[Vec3f],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    preprocess::positions_visible_into_scalar(
        CpuPositionView::new(positions),
        camera,
        depth_keys,
        indices,
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn preprocess_positions_visible_into_parallel(
    positions: &[Vec3f],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
    chunks: &mut Vec<PreprocessChunkScratch>,
) -> Result<(), RendererError> {
    preprocess::positions_visible_into(
        CpuPositionView::new(positions),
        camera,
        depth_keys,
        indices,
        chunks,
    )
}

pub(crate) fn preprocess_paged_visible_into(
    scene: &SceneBuffers,
    entries: &[(u32, u32)],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    preprocess::paged_visible_into(scene, entries, camera, depth_keys, indices)
}

#[cfg(test)]
pub(crate) fn world_to_camera_depth_with_view_row(
    pos_world: Vec3f,
    camera_position: Vec3f,
    depth_row: [f32; 3],
) -> f32 {
    preprocess::depth(pos_world, camera_position, depth_row)
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, Vec3f};

    use super::{
        CpuOrderEngine, depth_to_key, preprocess_positions_visible_into,
        world_to_camera_depth_with_view_row,
    };
    use crate::data::CpuPositionView;

    fn camera(near_plane: f32, far_plane: f32) -> Camera {
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = near_plane;
        camera.intrinsics.far_plane = far_plane;
        camera
    }

    #[test]
    fn scalar_oracle_covers_adversarial_values_and_257_tail() {
        let near = 1.0_f32;
        let far = 3.0_f32;
        let below_near = f32::from_bits(near.to_bits() - 1);
        let above_near = f32::from_bits(near.to_bits() + 1);
        let below_far = f32::from_bits(far.to_bits() - 1);
        let above_far = f32::from_bits(far.to_bits() + 1);
        let mut positions = vec![Vec3f::new(0.0, 0.0, 2.0); 257];
        positions[..10].copy_from_slice(&[
            Vec3f::new(0.0, 0.0, near),
            Vec3f::new(0.0, 0.0, far),
            Vec3f::new(0.0, 0.0, below_near),
            Vec3f::new(0.0, 0.0, above_near),
            Vec3f::new(0.0, 0.0, below_far),
            Vec3f::new(0.0, 0.0, above_far),
            Vec3f::new(0.0, 0.0, f32::NAN),
            Vec3f::new(0.0, 0.0, f32::INFINITY),
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::new(0.0, 0.0, -0.0),
        ]);

        let mut scalar_keys = Vec::new();
        let mut scalar_ids = Vec::new();
        preprocess_positions_visible_into(
            &positions,
            &camera(near, far),
            &mut scalar_keys,
            &mut scalar_ids,
        )
        .expect("scalar oracle");

        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::with_capacity(positions.len());
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(near, far),
                true,
                &mut order,
            )
            .expect("dispatched order");

        let mut expected = scalar_ids.clone();
        gsplat_sort::CpuSortBackend::default()
            .sort_values_by_keys(&scalar_keys, &mut expected)
            .expect("reference radix");
        assert_eq!(order, expected);
        assert!(order.contains(&0));
        assert!(order.contains(&1));
        assert!(!order.iter().any(|id| matches!(*id, 2 | 5 | 6 | 7 | 8 | 9)));
    }

    #[test]
    fn explicit_fma_depth_and_key_bits_are_authoritative() {
        let position = Vec3f::new(
            f32::from_bits(0x4b00_0001),
            f32::from_bits(0xcb00_0000),
            f32::from_bits(0x3f80_0001),
        );
        let camera_position = Vec3f::new(1.0, -1.0, f32::from_bits(1));
        let row = [
            f32::from_bits(0x3f00_0001),
            0.5,
            f32::from_bits(0x3f7f_ffff),
        ];
        let relative = [
            position.x - camera_position.x,
            position.y - camera_position.y,
            position.z - camera_position.z,
        ];
        let expected = row[2].mul_add(
            relative[2],
            row[1].mul_add(relative[1], row[0] * relative[0]),
        );
        let actual = world_to_camera_depth_with_view_row(position, camera_position, row);
        assert_eq!(actual.to_bits(), expected.to_bits());
        assert_eq!(depth_to_key(actual), actual.max(0.0).to_bits());
    }

    #[test]
    fn equal_depth_is_stable_source_id_order() {
        let positions = vec![Vec3f::new(0.0, 0.0, 2.0); 257];
        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::with_capacity(positions.len());
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("order");
        assert_eq!(order, (0..257_u32).collect::<Vec<_>>());
    }

    #[test]
    fn empty_single_and_invalid_camera_are_transactional() {
        let mut engine = CpuOrderEngine::try_with_capacity(1).expect("engine");
        let mut order = vec![99];
        engine
            .order_positions(
                CpuPositionView::new(&[]),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("empty");
        assert!(order.is_empty());

        let positions = [Vec3f::new(0.0, 0.0, 2.0)];
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("single");
        assert_eq!(order, [0]);

        let published = order.clone();
        assert!(
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(3.0, 1.0),
                    true,
                    &mut order,
                )
                .is_err()
        );
        assert_eq!(order, published);
    }

    #[test]
    fn warmup_reuses_bounded_double_buffers_without_reallocation() {
        let positions = (0..257)
            .map(|index| Vec3f::new(0.0, 0.0, 1.0 + index as f32 * 0.001))
            .collect::<Vec<_>>();
        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::new();
        order.reserve_exact(positions.len());

        for _ in 0..2 {
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(0.5, 4.0),
                    true,
                    &mut order,
                )
                .expect("warmup");
        }
        let first = (
            engine.buffer_state(),
            order.as_ptr() as usize,
            order.capacity(),
        );
        for _ in 0..4 {
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(0.5, 4.0),
                    true,
                    &mut order,
                )
                .expect("steady state");
        }
        let second = (
            engine.buffer_state(),
            order.as_ptr() as usize,
            order.capacity(),
        );
        assert_eq!(second, first);
    }

    #[test]
    fn scalar_output_growth_reaches_target_then_reuses_the_same_addresses() {
        let mut positions = vec![Vec3f::new(0.0, 0.0, 2.0); 64];
        let mut depth_keys = Vec::with_capacity(64);
        let mut source_ids = Vec::with_capacity(64);
        let camera = camera(1.0, 3.0);
        preprocess_positions_visible_into(&positions, &camera, &mut depth_keys, &mut source_ids)
            .expect("small warmup");

        positions.resize(100, Vec3f::new(0.0, 0.0, 2.0));
        preprocess_positions_visible_into(&positions, &camera, &mut depth_keys, &mut source_ids)
            .expect("grown input");
        assert!(depth_keys.capacity() >= positions.len());
        assert!(source_ids.capacity() >= positions.len());
        let grown = (
            depth_keys.as_ptr() as usize,
            depth_keys.capacity(),
            source_ids.as_ptr() as usize,
            source_ids.capacity(),
        );

        preprocess_positions_visible_into(&positions, &camera, &mut depth_keys, &mut source_ids)
            .expect("same-size reuse");
        assert_eq!(
            (
                depth_keys.as_ptr() as usize,
                depth_keys.capacity(),
                source_ids.as_ptr() as usize,
                source_ids.capacity(),
            ),
            grown
        );
    }
}
