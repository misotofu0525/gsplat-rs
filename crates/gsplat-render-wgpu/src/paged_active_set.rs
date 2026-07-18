//! Shared fixed-slot active-set ownership for the local paged prototype.
//!
//! This module owns GPU atlas residency and scheduling only. Page payloads are
//! still extracted from a complete in-memory `SceneBuffers`; a later source
//! boundary will replace that coupling without changing this owner.

use gsplat_core::{Camera, SceneBuffers};

use crate::{
    AttributeLod, DEFAULT_PAGED_ATLAS_SLOTS, PagedAtlasGpu, RendererError, ResidencyBudgets,
    ResidencyManager, SchedulerConfig, SchedulerView, SpatialPageSet, schedule_pages,
};

pub(crate) struct PagedActiveSet {
    pub(crate) pages: SpatialPageSet,
    pub(crate) atlas: PagedAtlasGpu,
    pub(crate) residency: ResidencyManager,
}

impl PagedActiveSet {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        pages: SpatialPageSet,
    ) -> Result<Self, RendererError> {
        let slot_count = pages.page_count().clamp(1, DEFAULT_PAGED_ATLAS_SLOTS);
        let atlas = PagedAtlasGpu::new(
            device,
            queue,
            layout,
            slot_count,
            pages.page_capacity,
            scene,
        )
        .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
        let residency = ResidencyManager::new(
            &pages,
            ResidencyBudgets {
                max_resident_pages: slot_count,
                max_inflight_pages: slot_count,
            },
        );
        Ok(Self {
            pages,
            atlas,
            residency,
        })
    }

    pub(crate) fn sync(
        &mut self,
        queue: &wgpu::Queue,
        scene: &SceneBuffers,
        camera: &Camera,
    ) -> Result<(), RendererError> {
        let previous = self.residency.resident_tokens();
        schedule_pages(
            &self.pages,
            &mut self.residency,
            SchedulerView {
                position: camera.pose.position,
            },
            SchedulerConfig {
                target_resident_pages: self.atlas.slot_count(),
                coarse_cover_radius: 2.0,
            },
        )
        .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
        let current = self.residency.resident_tokens();

        for token in previous {
            if !current.iter().any(|resident| {
                resident.page_id == token.page_id
                    && resident.slot == token.slot
                    && resident.slot_generation == token.slot_generation
            }) {
                self.atlas
                    .clear_slot(queue, token)
                    .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
            }
        }
        for token in current {
            if self.atlas.contains_token(token) {
                continue;
            }
            let page = self
                .pages
                .page(token.page_id)
                .ok_or(RendererError::InvalidScene)?;
            self.atlas
                .upload_page_if_current(
                    queue,
                    &self.residency,
                    token,
                    page,
                    scene,
                    AttributeLod::Degree0,
                )
                .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
        }
        Ok(())
    }
}
