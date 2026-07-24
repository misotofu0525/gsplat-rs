use gsplat_core::Camera;
#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::SceneBuffers;
use gsplat_sort::CpuSortBackend;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use crate::cpu::calibration::{
    CALIBRATION_INPUT_CAP, NativeCalibration, calibrate_bounded, native_candidates,
};
use crate::cpu::preprocess;
#[cfg(not(target_arch = "wasm32"))]
use crate::cpu::preprocess::{
    MAX_PARALLEL_PREPROCESS_CHUNKS,
    packed::{PackedPreprocessChunkScratch, PackedScalarExecution},
};
use crate::data::CpuPositionView;
use crate::{RendererError, timer_elapsed_ms, timer_now};

#[derive(Debug)]
pub(crate) struct WorkspaceAllocationError {
    pub(crate) resource: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkspaceTimings {
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
}

#[derive(Default)]
pub(crate) struct CpuOrderWorkspace {
    depth_keys: Vec<u32>,
    #[cfg(not(target_arch = "wasm32"))]
    packed_pairs: Vec<u64>,
    candidate_ids: Vec<u32>,
    sorter: CpuSortBackend,
    #[cfg(not(target_arch = "wasm32"))]
    packed_chunks: Vec<PackedPreprocessChunkScratch>,
    #[cfg(not(target_arch = "wasm32"))]
    native_calibration: NativeCalibration,
}

impl CpuOrderWorkspace {
    pub(crate) fn try_with_capacity(source_count: usize) -> Result<Self, WorkspaceAllocationError> {
        let mut workspace = Self::default();
        workspace
            .depth_keys
            .try_reserve_exact(source_count)
            .map_err(|_| WorkspaceAllocationError {
                resource: "depth keys",
            })?;
        workspace
            .candidate_ids
            .try_reserve_exact(source_count)
            .map_err(|_| WorkspaceAllocationError {
                resource: "working source IDs",
            })?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            workspace
                .packed_pairs
                .try_reserve_exact(source_count)
                .map_err(|_| WorkspaceAllocationError {
                    resource: "packed depth/source pairs",
                })?;
            workspace
                .packed_chunks
                .try_reserve_exact(MAX_PARALLEL_PREPROCESS_CHUNKS)
                .map_err(|_| WorkspaceAllocationError {
                    resource: "packed native chunk scratch",
                })?;
            workspace.packed_chunks.resize_with(
                MAX_PARALLEL_PREPROCESS_CHUNKS,
                PackedPreprocessChunkScratch::default,
            );
        }
        Ok(workspace)
    }

    pub(crate) fn order_positions(
        &mut self,
        positions: CpuPositionView<'_>,
        camera: &Camera,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
    ) -> Result<WorkspaceTimings, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            camera
                .validate()
                .map_err(|_| RendererError::InvalidCamera)?;
            let execution = self.calibrated_execution(positions, camera);
            let preprocess_start = timer_now();
            preprocess::packed::positions_visible_into(
                positions,
                camera,
                &mut self.packed_pairs,
                &mut self.packed_chunks,
                execution,
            )?;
            let preprocess_ms = timer_elapsed_ms(preprocess_start);
            self.finish_packed_order(stable_full32, authoritative_ids, preprocess_ms)
        }

        #[cfg(target_arch = "wasm32")]
        {
            let preprocess_start = timer_now();
            preprocess::positions_visible_into(
                positions,
                camera,
                &mut self.depth_keys,
                &mut self.candidate_ids,
            )?;
            let preprocess_ms = timer_elapsed_ms(preprocess_start);
            self.finish_order(stable_full32, authoritative_ids, preprocess_ms)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn calibrated_execution(
        &mut self,
        positions: CpuPositionView<'_>,
        camera: &Camera,
    ) -> PackedScalarExecution {
        if let Some(decision) = self.native_calibration.decision() {
            return decision.execution();
        }

        if positions.len() < preprocess::PARALLEL_PREPROCESS_THRESHOLD {
            return PackedScalarExecution::serial();
        }
        let candidates = native_candidates();
        let static_fallback = candidates.static_fallback();
        if let Some(decision) = candidates.singleton_decision() {
            debug_assert_eq!(static_fallback, PackedScalarExecution::serial());
            self.native_calibration.freeze_static(decision);
            return decision.execution();
        }
        let calibration_len = positions.len().min(CALIBRATION_INPUT_CAP);
        let calibration_positions = positions.slice(0..calibration_len);
        let started = Instant::now();
        let decision = calibrate_bounded(
            candidates,
            || started.elapsed(),
            |execution| self.probe_terminal_order(calibration_positions, camera, execution),
        );
        debug_assert!(
            decision.fallback_reason().is_none() || decision.execution() == static_fallback
        );
        self.native_calibration.freeze_after_probe(decision);
        decision.execution()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn probe_terminal_order(
        &mut self,
        positions: CpuPositionView<'_>,
        camera: &Camera,
        execution: PackedScalarExecution,
    ) -> Result<Duration, RendererError> {
        let started = Instant::now();
        preprocess::packed::positions_visible_into(
            positions,
            camera,
            &mut self.packed_pairs,
            &mut self.packed_chunks,
            execution,
        )?;
        self.candidate_ids.resize(self.packed_pairs.len(), 0);
        self.sorter
            .sort_prepacked_values(&mut self.packed_pairs, &mut self.candidate_ids)?;
        Ok(started.elapsed())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn order_paged(
        &mut self,
        scene: &SceneBuffers,
        entries: &[(u32, u32)],
        camera: &Camera,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
    ) -> Result<WorkspaceTimings, RendererError> {
        let preprocess_start = timer_now();
        preprocess::paged_visible_into(
            scene,
            entries,
            camera,
            &mut self.depth_keys,
            &mut self.candidate_ids,
        )?;
        let preprocess_ms = timer_elapsed_ms(preprocess_start);
        self.finish_order(stable_full32, authoritative_ids, preprocess_ms)
    }

    fn finish_order(
        &mut self,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
        preprocess_ms: f32,
    ) -> Result<WorkspaceTimings, RendererError> {
        let sort_start = timer_now();
        if stable_full32 {
            self.sorter
                .sort_values_by_keys(&self.depth_keys, &mut self.candidate_ids)?;
        }
        let sort_ms = timer_elapsed_ms(sort_start);

        // Publication is the final infallible step. Invalid cameras and sort
        // failures leave the caller's last authoritative order untouched.
        std::mem::swap(authoritative_ids, &mut self.candidate_ids);
        Ok(WorkspaceTimings {
            preprocess_ms,
            sort_ms,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn finish_packed_order(
        &mut self,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
        preprocess_ms: f32,
    ) -> Result<WorkspaceTimings, RendererError> {
        let sort_start = timer_now();
        self.candidate_ids.resize(self.packed_pairs.len(), 0);
        if stable_full32 {
            self.sorter
                .sort_prepacked_values(&mut self.packed_pairs, &mut self.candidate_ids)?;
        } else {
            for (source_id, pair) in self.candidate_ids.iter_mut().zip(&self.packed_pairs) {
                *source_id = !(*pair as u32);
            }
        }
        let sort_ms = timer_elapsed_ms(sort_start);

        // Publication is the final infallible step. Invalid cameras and sort
        // failures leave the caller's last authoritative order untouched.
        std::mem::swap(authoritative_ids, &mut self.candidate_ids);
        Ok(WorkspaceTimings {
            preprocess_ms,
            sort_ms,
        })
    }

    #[cfg(test)]
    pub(crate) fn buffer_state(&self) -> ((usize, usize), (usize, usize), (usize, usize)) {
        #[cfg(not(target_arch = "wasm32"))]
        let packed_state = (
            self.packed_pairs.as_ptr() as usize,
            self.packed_pairs.capacity(),
        );
        #[cfg(target_arch = "wasm32")]
        let packed_state = (0, 0);

        (
            packed_state,
            (
                self.depth_keys.as_ptr() as usize,
                self.depth_keys.capacity(),
            ),
            (
                self.candidate_ids.as_ptr() as usize,
                self.candidate_ids.capacity(),
            ),
        )
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn calibration_state(
        &self,
    ) -> (Option<crate::cpu::calibration::CalibrationDecision>, usize) {
        (
            self.native_calibration.decision(),
            self.native_calibration.attempts(),
        )
    }
}
