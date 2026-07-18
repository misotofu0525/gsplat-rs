//! Shared fixed-slot active-set ownership for the local paged prototype.
//!
//! This module owns GPU atlas residency and scheduling. The current caller
//! still supplies a complete in-memory `SceneBuffers` through the explicitly
//! local page-source adapter; only decoded payloads reach GPU upload.

use gsplat_core::{Camera, SceneBuffers};

use crate::page_source::{LocalScenePageSource, PageEncoding, PageSource};
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
        let atlas = PagedAtlasGpu::new_with_encoding(
            device,
            queue,
            layout,
            slot_count,
            pages.page_capacity,
            PageEncoding::from_scene(scene),
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
        let source = LocalScenePageSource::new(scene, &self.pages, self.atlas.encoding());
        Self::sync_from_source(queue, &mut self.residency, &mut self.atlas, &source, camera)
    }

    fn sync_from_source(
        queue: &wgpu::Queue,
        residency: &mut ResidencyManager,
        atlas: &mut PagedAtlasGpu,
        source: &impl PageSource,
        camera: &Camera,
    ) -> Result<(), RendererError> {
        let previous = residency.resident_tokens();
        schedule_pages(
            source.pages(),
            residency,
            SchedulerView {
                position: camera.pose.position,
            },
            SchedulerConfig {
                target_resident_pages: atlas.slot_count(),
                coarse_cover_radius: 2.0,
            },
        )
        .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
        let current = residency.resident_tokens();

        for token in previous {
            if !current.iter().any(|resident| {
                resident.page_id == token.page_id
                    && resident.slot == token.slot
                    && resident.slot_generation == token.slot_generation
            }) {
                atlas
                    .clear_slot(queue, token)
                    .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
            }
        }
        for token in current {
            if atlas.contains_token(token) {
                continue;
            }
            let payload = source
                .decode_page(token.page_id, AttributeLod::Degree0)
                .ok_or(RendererError::InvalidScene)?;
            atlas
                .upload_decoded_page_if_current(queue, residency, token, &payload)
                .map_err(|err| RendererError::PagedAtlas(format!("{err:?}")))?;
        }
        Ok(())
    }
}
