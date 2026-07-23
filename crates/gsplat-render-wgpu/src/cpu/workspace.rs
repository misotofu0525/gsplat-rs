use gsplat_core::Camera;
#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::SceneBuffers;
use gsplat_sort::CpuSortBackend;

use crate::cpu::preprocess;
#[cfg(not(target_arch = "wasm32"))]
use crate::cpu::preprocess::{MAX_PARALLEL_PREPROCESS_CHUNKS, PreprocessChunkScratch};
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
    candidate_ids: Vec<u32>,
    sorter: CpuSortBackend,
    #[cfg(not(target_arch = "wasm32"))]
    native_chunks: Vec<PreprocessChunkScratch>,
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
                .native_chunks
                .try_reserve_exact(MAX_PARALLEL_PREPROCESS_CHUNKS)
                .map_err(|_| WorkspaceAllocationError {
                    resource: "native chunk scratch",
                })?;
            workspace.native_chunks.resize_with(
                MAX_PARALLEL_PREPROCESS_CHUNKS,
                PreprocessChunkScratch::default,
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
        let preprocess_start = timer_now();
        preprocess::positions_visible_into(
            positions,
            camera,
            &mut self.depth_keys,
            &mut self.candidate_ids,
            #[cfg(not(target_arch = "wasm32"))]
            &mut self.native_chunks,
        )?;
        let preprocess_ms = timer_elapsed_ms(preprocess_start);
        self.finish_order(stable_full32, authoritative_ids, preprocess_ms)
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

    #[cfg(test)]
    pub(crate) fn buffer_state(&self) -> ((usize, usize), (usize, usize)) {
        (
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
}
