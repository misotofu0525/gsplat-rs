//! GPU upload helpers for Phase D paged atlas slots.
//!
//! Pages are packed against shared scene encoding ranges and written into a
//! fixed-capacity [`PackedAtlasResources`] buffer at
//! `atlas_slot * page_capacity`. This keeps the existing packed shader's single
//! bounds/log-scale uniform valid across resident pages.

use gsplat_core::SceneBuffers;

use crate::DirectSceneError;
use crate::packed_atlas::{
    LogScaleRange, PackedAtlasCpuBuffers, PackedHotRecord, PackedSceneCpu, PackedShSidecar,
    SceneBounds, pack_scene_with_encoding, scene_sh_scales,
};
use crate::packed_gpu::PackedAtlasResources;
use crate::page_atlas::extract_page_scene;
use crate::residency::{AsyncPageToken, AttributeLod, ResidencyManager};
use crate::spatial_pages::{PageId, SpatialPage};

#[derive(Debug)]
pub enum PagedGpuError {
    Direct(DirectSceneError),
    SlotOutOfRange,
    StaleToken,
    PageTooLarge {
        splat_count: usize,
        page_capacity: usize,
    },
    EmptyPage,
}

impl From<DirectSceneError> for PagedGpuError {
    fn from(value: DirectSceneError) -> Self {
        Self::Direct(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GpuSlotMeta {
    page_id: Option<PageId>,
    generation: u64,
    splat_count: usize,
}

impl GpuSlotMeta {
    fn empty(generation: u64) -> Self {
        Self {
            page_id: None,
            generation,
            splat_count: 0,
        }
    }

    fn is_occupied(&self) -> bool {
        self.page_id.is_some() && self.splat_count > 0
    }
}

/// Fixed-capacity GPU atlas that hosts multiple resident spatial pages.
pub struct PagedAtlasGpu {
    pub page_capacity: usize,
    pub resources: PackedAtlasResources,
    slot_meta: Vec<GpuSlotMeta>,
    /// Scene splat indices stored per atlas slot (length `page_capacity` when occupied).
    slot_scene_indices: Vec<Vec<u32>>,
    scene_bounds: SceneBounds,
    log_scale_range: LogScaleRange,
    sh_scales: [f32; 3],
}

impl PagedAtlasGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        slot_count: usize,
        page_capacity: usize,
        scene: &SceneBuffers,
    ) -> Result<Self, PagedGpuError> {
        let slot_count = slot_count.max(1);
        let page_capacity = page_capacity.max(1);
        let capacity = slot_count
            .checked_mul(page_capacity)
            .ok_or(PagedGpuError::Direct(
                DirectSceneError::SortedIndexCapacityExceeded,
            ))?;
        let scene_bounds = SceneBounds::from_positions(&scene.positions);
        let log_scale_range = LogScaleRange::from_scales(&scene.scale_xyz);
        let sh_scales = scene_sh_scales(scene);
        let placeholder = placeholder_packed(capacity, scene_bounds, log_scale_range, sh_scales);
        let resources = PackedAtlasResources::new(device, queue, bind_group_layout, &placeholder)?;
        Ok(Self {
            page_capacity,
            resources,
            slot_meta: (0..slot_count).map(|_| GpuSlotMeta::empty(1)).collect(),
            slot_scene_indices: (0..slot_count).map(|_| Vec::new()).collect(),
            scene_bounds,
            log_scale_range,
            sh_scales,
        })
    }

    pub fn slot_count(&self) -> usize {
        self.slot_meta.len()
    }

    pub fn occupied_slot_count(&self) -> usize {
        self.slot_meta
            .iter()
            .filter(|slot| slot.is_occupied())
            .count()
    }

    pub fn contains_token(&self, token: AsyncPageToken) -> bool {
        self.slot_meta.get(token.slot as usize).is_some_and(|slot| {
            slot.is_occupied()
                && slot.page_id == Some(token.page_id)
                && slot.generation == token.slot_generation
        })
    }

    pub fn resident_splat_count(&self) -> usize {
        self.slot_meta
            .iter()
            .filter(|slot| slot.is_occupied())
            .map(|meta| meta.splat_count)
            .sum()
    }

    /// Global atlas indices for all occupied page splats (stable slot order).
    pub fn active_global_indices(&self) -> Vec<u32> {
        self.active_entries()
            .into_iter()
            .map(|(global, _)| global)
            .collect()
    }

    /// `(global_atlas_index, scene_splat_index)` pairs for resident splats.
    pub fn active_entries(&self) -> Vec<(u32, u32)> {
        let mut entries = Vec::with_capacity(self.resident_splat_count());
        for (slot, meta) in self.slot_meta.iter().enumerate() {
            if !meta.is_occupied() {
                continue;
            }
            let base = (slot * self.page_capacity) as u32;
            let scene_indices = &self.slot_scene_indices[slot];
            for local in 0..meta.splat_count {
                let scene_index = scene_indices.get(local).copied().unwrap_or(0);
                entries.push((base + local as u32, scene_index));
            }
        }
        entries
    }

    /// Install a page without a residency token (bootstrap / full-resident load).
    pub fn force_install_page(
        &mut self,
        queue: &wgpu::Queue,
        atlas_slot: u32,
        page: &SpatialPage,
        scene: &SceneBuffers,
        attribute_lod: AttributeLod,
    ) -> Result<(), PagedGpuError> {
        let generation = self
            .slot_meta
            .get(atlas_slot as usize)
            .map(|slot| slot.generation)
            .ok_or(PagedGpuError::SlotOutOfRange)?;
        self.upload_page(
            queue,
            AsyncPageToken {
                scene_revision: 0,
                page_id: page.id,
                slot: atlas_slot,
                slot_generation: generation,
            },
            page,
            scene,
            attribute_lod,
        )
    }

    pub fn upload_page(
        &mut self,
        queue: &wgpu::Queue,
        token: AsyncPageToken,
        page: &SpatialPage,
        scene: &SceneBuffers,
        attribute_lod: AttributeLod,
    ) -> Result<(), PagedGpuError> {
        let slot = self
            .slot_meta
            .get_mut(token.slot as usize)
            .ok_or(PagedGpuError::SlotOutOfRange)?;
        if slot.is_occupied() {
            if slot.generation != token.slot_generation || slot.page_id != Some(token.page_id) {
                return Err(PagedGpuError::StaleToken);
            }
        } else if slot.generation > token.slot_generation {
            return Err(PagedGpuError::StaleToken);
        } else {
            slot.generation = token.slot_generation;
        }
        if page.splat_count() == 0 {
            return Err(PagedGpuError::EmptyPage);
        }
        if page.splat_count() > self.page_capacity {
            return Err(PagedGpuError::PageTooLarge {
                splat_count: page.splat_count(),
                page_capacity: self.page_capacity,
            });
        }

        let extracted = extract_page_scene(scene, page);
        let mut packed = pack_scene_with_encoding(
            &extracted,
            self.scene_bounds,
            self.log_scale_range,
            Some(self.sh_scales),
        );
        if attribute_lod == AttributeLod::Degree0 {
            packed.sh_degree = 0;
            packed.sh_sidecars.clear();
        }
        let words = PackedAtlasCpuBuffers::hot_storage_words(&packed);
        let base = token.slot as usize * self.page_capacity;
        // Clear the whole page slot first so stale trailing splats disappear.
        self.resources
            .clear_hot_records_range(queue, base, self.page_capacity);
        self.resources.write_hot_records_at(queue, base, &words);
        self.slot_scene_indices[token.slot as usize] = page.splat_indices.clone();
        *slot = GpuSlotMeta {
            page_id: Some(token.page_id),
            generation: token.slot_generation,
            splat_count: page.splat_count(),
        };
        Ok(())
    }

    pub fn upload_page_if_current(
        &mut self,
        queue: &wgpu::Queue,
        manager: &ResidencyManager,
        token: AsyncPageToken,
        page: &SpatialPage,
        scene: &SceneBuffers,
        attribute_lod: AttributeLod,
    ) -> Result<(), PagedGpuError> {
        manager
            .validate_token(token)
            .map_err(|_| PagedGpuError::StaleToken)?;
        self.upload_page(queue, token, page, scene, attribute_lod)
    }

    pub fn clear_slot(
        &mut self,
        queue: &wgpu::Queue,
        token: AsyncPageToken,
    ) -> Result<(), PagedGpuError> {
        let slot = self
            .slot_meta
            .get_mut(token.slot as usize)
            .ok_or(PagedGpuError::SlotOutOfRange)?;
        if slot.page_id != Some(token.page_id) || slot.generation != token.slot_generation {
            return Err(PagedGpuError::StaleToken);
        }
        let base = token.slot as usize * self.page_capacity;
        self.resources
            .clear_hot_records_range(queue, base, self.page_capacity);
        self.slot_scene_indices[token.slot as usize].clear();
        *slot = GpuSlotMeta::empty(slot.generation.saturating_add(1));
        Ok(())
    }
}

fn placeholder_packed(
    capacity: usize,
    bounds: SceneBounds,
    log_scale_range: LogScaleRange,
    sh_scales: [f32; 3],
) -> PackedSceneCpu {
    let zero = PackedHotRecord {
        position_opacity: [0; 4],
        scale_flags: 0,
        rotation: 0,
        color: 0,
    };
    PackedSceneCpu {
        bounds,
        log_scale_range,
        hot: vec![zero; capacity.max(1)],
        sh_sidecars: vec![
            PackedShSidecar {
                coeffs_i8: [0; 45],
                pad: [0; 3],
            };
            capacity.max(1)
        ],
        sh_scales,
        sh_degree: 0,
        splat_count: capacity.max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packed_gpu::create_packed_bind_group_layout;
    use crate::residency::{ResidencyBudgets, ResidencyManager};
    use crate::spatial_pages::partition_scene_pages;
    use crate::{GeometryPath, Renderer, RendererConfig, RendererError};
    use gsplat_core::{Camera, RenderMode, Vec3f};

    fn sample_scene(count: usize) -> SceneBuffers {
        SceneBuffers {
            positions: (0..count)
                .map(|i| Vec3f::new((i as f32 - 3.5) * 0.15, 0.0, 1.2))
                .collect(),
            opacity: vec![2.0; count],
            scale_xyz: vec![[-3.0, -3.0, -3.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.2, -0.05, 0.1]; count],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn paged_gpu_upload_matches_whole_scene_packed_counts() {
        let scene = sample_scene(8);
        let pages = partition_scene_pages(&scene, 3, 2);
        let config = RendererConfig {
            width: 128,
            height: 128,
            mode: RenderMode::SortedAlpha,
        };
        let mut whole = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(RendererError::GpuRasterizerUnavailable)
            | Err(RendererError::GpuDeviceCreation) => {
                eprintln!("skipping paged GPU upload test; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        whole.set_geometry_path(GeometryPath::PackedAtlas);
        whole.load_scene(scene.clone()).unwrap();

        let device = whole
            .device()
            .expect("offscreen renderer has a device")
            .clone();
        let queue = whole
            .queue()
            .expect("offscreen renderer has a queue")
            .clone();
        let layout = create_packed_bind_group_layout(&device);
        let mut manager = ResidencyManager::new(
            &pages,
            ResidencyBudgets {
                max_resident_pages: pages.page_count().max(1),
                max_inflight_pages: pages.page_count().max(1),
            },
        );
        let mut paged = PagedAtlasGpu::new(
            &device,
            &queue,
            &layout,
            pages.page_count().max(1),
            3,
            &scene,
        )
        .expect("allocate paged GPU atlas");

        for page in &pages.pages {
            let token = manager.request_page(page.id).unwrap();
            manager.advance_to_compressed_ready(token).unwrap();
            manager.advance_to_decoded_ready(token).unwrap();
            manager.advance_to_uploading(token).unwrap();
            paged
                .upload_page_if_current(
                    &queue,
                    &manager,
                    token,
                    page,
                    &scene,
                    AttributeLod::Degree0,
                )
                .unwrap();
            manager.complete_resident(token).unwrap();
        }

        assert_eq!(paged.resident_splat_count(), scene.len());
        assert_eq!(paged.active_global_indices().len(), scene.len());

        let camera = Camera::default();
        let whole_stats = whole.render_frame(&camera).unwrap();
        // Drive the paged atlas through the same packed prepare path.
        let indices = paged.active_global_indices();
        let instance_count = paged
            .resources
            .prepare(&queue, &indices, &camera, config.width, config.height, true)
            .unwrap();
        assert_eq!(instance_count as usize, indices.len());
        assert!(paged.occupied_slot_count() >= 1);
        eprintln!(
            "paged GPU upload: pages={} resident_splats={} whole_visible={} whole_drawn={}",
            pages.page_count(),
            paged.resident_splat_count(),
            whole_stats.visible_count,
            whole_stats.drawn_count
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn paged_gpu_rejects_stale_upload_after_clear() {
        let scene = sample_scene(4);
        let pages = partition_scene_pages(&scene, 2, 2);
        let config = RendererConfig {
            width: 64,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };
        let renderer = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(RendererError::GpuRasterizerUnavailable)
            | Err(RendererError::GpuDeviceCreation) => {
                eprintln!("skipping paged GPU stale-token test; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let device = renderer
            .device()
            .expect("offscreen renderer has a device")
            .clone();
        let queue = renderer
            .queue()
            .expect("offscreen renderer has a queue")
            .clone();
        let layout = create_packed_bind_group_layout(&device);
        let mut manager = ResidencyManager::new(
            &pages,
            ResidencyBudgets {
                max_resident_pages: 2,
                max_inflight_pages: 2,
            },
        );
        let mut paged = PagedAtlasGpu::new(&device, &queue, &layout, 2, 2, &scene).unwrap();
        let page = &pages.pages[0];
        let token = manager.request_page(page.id).unwrap();
        manager.advance_to_compressed_ready(token).unwrap();
        manager.advance_to_decoded_ready(token).unwrap();
        manager.advance_to_uploading(token).unwrap();
        paged
            .upload_page(&queue, token, page, &scene, AttributeLod::Degree0)
            .unwrap();
        manager.complete_resident(token).unwrap();

        let evict = manager.begin_evict(page.id).unwrap();
        paged.clear_slot(&queue, evict).unwrap();
        manager.finish_evict(evict).unwrap();
        assert!(matches!(
            paged.upload_page_if_current(
                &queue,
                &manager,
                token,
                page,
                &scene,
                AttributeLod::Degree0,
            ),
            Err(PagedGpuError::StaleToken)
        ));
        assert!(paged.active_entries().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn paged_gpu_cancelled_token_never_enters_active_draw_set() {
        let scene = sample_scene(4);
        let pages = partition_scene_pages(&scene, 2, 2);
        let config = RendererConfig {
            width: 64,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };
        let renderer = match Renderer::with_config(config) {
            Ok(renderer) => renderer,
            Err(RendererError::GpuRasterizerUnavailable)
            | Err(RendererError::GpuDeviceCreation) => {
                eprintln!("skipping paged GPU cancel test; adapter unavailable");
                return;
            }
            Err(error) => panic!("renderer init: {error}"),
        };
        let device = renderer.device().unwrap().clone();
        let queue = renderer.queue().unwrap().clone();
        let layout = create_packed_bind_group_layout(&device);
        let mut manager = ResidencyManager::new(
            &pages,
            ResidencyBudgets {
                max_resident_pages: 1,
                max_inflight_pages: 1,
            },
        );
        let mut paged = PagedAtlasGpu::new(&device, &queue, &layout, 1, 2, &scene).unwrap();
        let page = &pages.pages[0];
        let token = manager.request_page(page.id).unwrap();
        manager.advance_to_compressed_ready(token).unwrap();
        manager.advance_to_decoded_ready(token).unwrap();
        manager.advance_to_uploading(token).unwrap();
        manager.cancel_inflight(token).unwrap();

        assert!(matches!(
            paged.upload_page_if_current(
                &queue,
                &manager,
                token,
                page,
                &scene,
                AttributeLod::Degree0,
            ),
            Err(PagedGpuError::StaleToken)
        ));
        assert!(paged.active_entries().is_empty());
    }
}
