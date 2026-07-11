//! CPU-side page atlas staging for Phase D.
//!
//! Resident pages are packed into fixed atlas slots using the Phase B hot
//! record. This slice validates extract/pack/install/clear and average
//! attribute-byte accounting before GPU upload wiring.

use gsplat_core::SceneBuffers;

use crate::packed_atlas::{
    FULL_DEGREE3_ATTRIBUTE_BYTES, HOT_RECORD_BYTES, PackedSceneCpu, pack_scene,
};
use crate::residency::{AsyncPageToken, AttributeLod};
use crate::spatial_pages::{PageId, SpatialPage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAtlasError {
    SlotOutOfRange,
    StaleToken,
    PageTooLarge {
        splat_count: usize,
        page_capacity: usize,
    },
    EmptyPage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageAtlasSlotCpu {
    pub page_id: Option<PageId>,
    pub generation: u64,
    pub packed: Option<PackedSceneCpu>,
    pub attribute_lod: AttributeLod,
}

impl PageAtlasSlotCpu {
    fn empty(generation: u64) -> Self {
        Self {
            page_id: None,
            generation,
            packed: None,
            attribute_lod: AttributeLod::Degree0,
        }
    }

    pub fn is_occupied(&self) -> bool {
        self.packed.is_some()
    }

    pub fn attribute_bytes(&self) -> u64 {
        self.packed
            .as_ref()
            .map(PackedSceneCpu::total_attribute_bytes)
            .unwrap_or(0)
    }
}

/// Fixed slot table holding packed page payloads on the CPU.
#[derive(Debug, Clone, PartialEq)]
pub struct PageAtlasCpu {
    pub page_capacity: usize,
    slots: Vec<PageAtlasSlotCpu>,
}

impl PageAtlasCpu {
    pub fn new(slot_count: usize, page_capacity: usize) -> Self {
        let slot_count = slot_count.max(1);
        let page_capacity = page_capacity.max(1);
        Self {
            page_capacity,
            slots: (0..slot_count)
                .map(|_| PageAtlasSlotCpu::empty(1))
                .collect(),
        }
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, index: u32) -> Option<&PageAtlasSlotCpu> {
        self.slots.get(index as usize)
    }

    pub fn occupied_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_occupied()).count()
    }

    pub fn total_attribute_bytes(&self) -> u64 {
        self.slots
            .iter()
            .map(PageAtlasSlotCpu::attribute_bytes)
            .sum()
    }

    pub fn total_resident_splats(&self) -> usize {
        self.slots
            .iter()
            .filter_map(|slot| slot.packed.as_ref())
            .map(|packed| packed.splat_count)
            .sum()
    }

    /// Average attribute bytes per resident splat before order/scratch.
    pub fn average_attribute_bytes_per_splat(&self) -> Option<f64> {
        let splats = self.total_resident_splats();
        if splats == 0 {
            None
        } else {
            Some(self.total_attribute_bytes() as f64 / splats as f64)
        }
    }

    pub fn install_page(
        &mut self,
        token: AsyncPageToken,
        page: &SpatialPage,
        scene: &SceneBuffers,
        attribute_lod: AttributeLod,
    ) -> Result<(), PageAtlasError> {
        let slot = self
            .slots
            .get_mut(token.slot as usize)
            .ok_or(PageAtlasError::SlotOutOfRange)?;
        if slot.packed.is_some() {
            if slot.generation != token.slot_generation || slot.page_id != Some(token.page_id) {
                return Err(PageAtlasError::StaleToken);
            }
        } else if slot.generation > token.slot_generation {
            // Slot was cleared/reused ahead of this token.
            return Err(PageAtlasError::StaleToken);
        } else {
            // Adopt the residency manager generation for an empty slot.
            slot.generation = token.slot_generation;
        }
        if page.splat_count() == 0 {
            return Err(PageAtlasError::EmptyPage);
        }
        if page.splat_count() > self.page_capacity {
            return Err(PageAtlasError::PageTooLarge {
                splat_count: page.splat_count(),
                page_capacity: self.page_capacity,
            });
        }
        let extracted = extract_page_scene(scene, page);
        let mut packed = pack_scene(&extracted);
        apply_attribute_lod(&mut packed, attribute_lod);
        slot.page_id = Some(token.page_id);
        slot.packed = Some(packed);
        slot.attribute_lod = attribute_lod;
        Ok(())
    }

    pub fn clear_slot(&mut self, token: AsyncPageToken) -> Result<(), PageAtlasError> {
        let slot = self
            .slots
            .get_mut(token.slot as usize)
            .ok_or(PageAtlasError::SlotOutOfRange)?;
        if slot.generation != token.slot_generation {
            return Err(PageAtlasError::StaleToken);
        }
        if slot.page_id != Some(token.page_id) {
            return Err(PageAtlasError::StaleToken);
        }
        *slot = PageAtlasSlotCpu::empty(slot.generation.saturating_add(1));
        Ok(())
    }
}

/// Build a `SceneBuffers` containing only the splats listed by `page`.
pub fn extract_page_scene(scene: &SceneBuffers, page: &SpatialPage) -> SceneBuffers {
    let count = page.splat_count();
    let mut positions = Vec::with_capacity(count);
    let mut opacity = Vec::with_capacity(count);
    let mut scale_xyz = Vec::with_capacity(count);
    let mut rotation_xyzw = Vec::with_capacity(count);
    let mut color_dc = Vec::with_capacity(count);
    let coeffs_per_splat = match scene.sh_degree {
        0 => 0,
        1 => 9,
        2 => 24,
        _ => 45,
    };
    let mut sh_rest = if coeffs_per_splat == 0 {
        None
    } else {
        Some(Vec::with_capacity(count * coeffs_per_splat))
    };

    for &index in &page.splat_indices {
        let index = index as usize;
        positions.push(scene.positions[index]);
        opacity.push(scene.opacity[index]);
        scale_xyz.push(scene.scale_xyz[index]);
        rotation_xyzw.push(scene.rotation_xyzw[index]);
        color_dc.push(scene.color_dc[index]);
        if let (Some(dst), Some(src)) = (sh_rest.as_mut(), scene.sh_rest.as_ref()) {
            let base = index * coeffs_per_splat;
            if base + coeffs_per_splat <= src.len() {
                dst.extend_from_slice(&src[base..base + coeffs_per_splat]);
            } else {
                dst.extend(std::iter::repeat_n(0.0, coeffs_per_splat));
            }
        }
    }

    SceneBuffers {
        positions,
        opacity,
        scale_xyz,
        rotation_xyzw,
        color_dc,
        sh_degree: scene.sh_degree,
        sh_rest,
    }
}

fn apply_attribute_lod(packed: &mut PackedSceneCpu, lod: AttributeLod) {
    packed.sh_degree = lod.as_u8().min(packed.sh_degree);
    if packed.sh_degree == 0 {
        packed.sh_sidecars.clear();
        packed.sh_scales = [0.0; 3];
    }
}

/// Nominal attribute bytes for a chosen LOD (before order/scratch).
pub fn attribute_bytes_for_lod(lod: AttributeLod) -> usize {
    match lod {
        AttributeLod::Degree0 => HOT_RECORD_BYTES,
        AttributeLod::Degree1 | AttributeLod::Degree2 | AttributeLod::Degree3 => {
            FULL_DEGREE3_ATTRIBUTE_BYTES
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_scheduler::{SchedulerConfig, SchedulerView, schedule_pages};
    use crate::residency::{PageResidencyState, ResidencyBudgets, ResidencyManager};
    use crate::spatial_pages::partition_scene_pages;
    use gsplat_core::Vec3f;

    fn sample_scene(count: usize, sh_degree: u8) -> SceneBuffers {
        let coeffs = match sh_degree {
            0 => 0,
            1 => 9,
            2 => 24,
            _ => 45,
        };
        SceneBuffers {
            positions: (0..count)
                .map(|i| Vec3f::new(i as f32 * 0.5, 0.0, 1.0))
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-3.0, -3.0, -3.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.1, 0.0, -0.1]; count],
            sh_degree,
            sh_rest: if coeffs == 0 {
                None
            } else {
                Some(vec![0.05; count * coeffs])
            },
        }
    }

    #[test]
    fn extract_page_scene_preserves_count_and_sh_layout() {
        let scene = sample_scene(8, 3);
        let pages = partition_scene_pages(&scene, 3, 2);
        let page = &pages.pages[0];
        let extracted = extract_page_scene(&scene, page);
        assert_eq!(extracted.len(), page.splat_count());
        assert_eq!(extracted.sh_degree, 3);
        assert_eq!(
            extracted.sh_rest.as_ref().map(Vec::len),
            Some(page.splat_count() * 45)
        );
    }

    #[test]
    fn install_and_clear_respect_slot_generation() {
        let scene = sample_scene(6, 0);
        let pages = partition_scene_pages(&scene, 2, 3);
        let mut manager = ResidencyManager::new(
            &pages,
            ResidencyBudgets {
                max_resident_pages: 4,
                max_inflight_pages: 4,
            },
        );
        let mut atlas = PageAtlasCpu::new(4, 2);
        let page = &pages.pages[0];
        let token = manager.request_page(page.id).unwrap();
        manager.advance_to_compressed_ready(token).unwrap();
        manager.advance_to_decoded_ready(token).unwrap();
        manager.advance_to_uploading(token).unwrap();
        atlas
            .install_page(token, page, &scene, AttributeLod::Degree0)
            .unwrap();
        manager.complete_resident(token).unwrap();
        assert_eq!(atlas.occupied_count(), 1);
        assert!(atlas.average_attribute_bytes_per_splat().unwrap() <= HOT_RECORD_BYTES as f64);

        let evict = manager.begin_evict(page.id).unwrap();
        atlas.clear_slot(evict).unwrap();
        manager.finish_evict(evict).unwrap();
        assert_eq!(atlas.occupied_count(), 0);

        // Stale install after generation bump must fail.
        assert_eq!(
            atlas.install_page(token, page, &scene, AttributeLod::Degree0),
            Err(PageAtlasError::StaleToken)
        );
    }

    #[test]
    fn degree0_mix_keeps_average_attribute_bytes_in_hot_band() {
        let scene = sample_scene(12, 3);
        let pages = partition_scene_pages(&scene, 2, 4);
        let mut manager = ResidencyManager::new(
            &pages,
            ResidencyBudgets {
                max_resident_pages: 4,
                max_inflight_pages: 4,
            },
        );
        let mut atlas = PageAtlasCpu::new(4, 2);
        let outcome = schedule_pages(
            &pages,
            &mut manager,
            SchedulerView {
                position: Vec3f::new(0.0, 0.0, 1.0),
            },
            SchedulerConfig {
                target_resident_pages: 3,
                coarse_cover_radius: 8.0,
            },
        )
        .unwrap();

        let mut installed = 0_usize;
        for page_id in manager.resident_page_ids() {
            let page = pages.page(page_id).unwrap();
            let residency = manager.page(page_id).unwrap();
            assert_eq!(residency.state, PageResidencyState::Resident);
            let token = AsyncPageToken {
                scene_revision: manager.scene_revision(),
                page_id,
                slot: residency.slot.unwrap(),
                slot_generation: residency.slot_generation,
            };
            manager
                .set_attribute_lod(page_id, AttributeLod::Degree0)
                .unwrap();
            atlas
                .install_page(token, page, &scene, AttributeLod::Degree0)
                .unwrap();
            installed += 1;
        }
        assert!(
            installed > 0,
            "scheduler must residentially cover near pages"
        );
        assert!(!outcome.retained.is_empty());

        let average = atlas.average_attribute_bytes_per_splat().unwrap();
        assert!(
            (20.0..48.0).contains(&average),
            "average attribute bytes {average} should land in the 20-48 design band"
        );
        assert_eq!(average, HOT_RECORD_BYTES as f64);
    }
}
