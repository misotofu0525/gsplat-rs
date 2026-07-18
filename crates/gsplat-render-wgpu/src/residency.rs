//! Page residency state machine and slot-generation validation for Phase D.
//!
//! Asynchronous decode/upload results must carry `scene_revision`, `page_id`,
//! and `slot_generation`. Stale results are rejected before they can mutate
//! atlas slots or published order.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::spatial_pages::{PageId, SpatialPageSet};

/// Residency lifecycle for one spatial page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageResidencyState {
    Absent,
    Requested,
    CompressedReady,
    DecodedReady,
    Uploading,
    Resident,
    Evicting,
}

/// Attribute LOD selected independently from spatial residency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AttributeLod {
    Degree0 = 0,
    Degree1 = 1,
    Degree2 = 2,
    Degree3 = 3,
}

impl AttributeLod {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Degree0),
            1 => Some(Self::Degree1),
            2 => Some(Self::Degree2),
            3 => Some(Self::Degree3),
            _ => None,
        }
    }
}

/// One atlas slot that may be reused across pages over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasSlot {
    pub index: u32,
    pub generation: u64,
    pub page_id: Option<PageId>,
}

/// Per-page residency record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResidency {
    pub page_id: PageId,
    pub state: PageResidencyState,
    pub slot: Option<u32>,
    pub slot_generation: u64,
    pub attribute_lod: AttributeLod,
    pub last_visible_frame: u64,
}

/// Token returned by async decode/upload work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsyncPageToken {
    pub scene_revision: u64,
    pub page_id: PageId,
    pub slot: u32,
    pub slot_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidencyError {
    UnknownPage,
    NoFreeSlot,
    InvalidTransition {
        from: PageResidencyState,
        to: PageResidencyState,
    },
    StaleAsyncResult,
    SlotMismatch,
    BudgetExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResidencyBudgets {
    pub max_resident_pages: usize,
    pub max_inflight_pages: usize,
}

impl Default for ResidencyBudgets {
    fn default() -> Self {
        Self {
            max_resident_pages: 64,
            max_inflight_pages: 8,
        }
    }
}

/// Owns page residency transitions and slot generations.
#[derive(Debug)]
pub struct ResidencyManager {
    scene_revision: u64,
    budgets: ResidencyBudgets,
    pages: HashMap<u32, PageResidency>,
    slots: Vec<AtlasSlot>,
    free_slots: VecDeque<u32>,
    frame_index: u64,
}

impl ResidencyManager {
    pub fn new(pages: &SpatialPageSet, budgets: ResidencyBudgets) -> Self {
        let slot_count = budgets.max_resident_pages.max(1);
        let mut slots = Vec::with_capacity(slot_count);
        let mut free_slots = VecDeque::with_capacity(slot_count);
        for index in 0..slot_count {
            slots.push(AtlasSlot {
                index: index as u32,
                generation: 1,
                page_id: None,
            });
            free_slots.push_back(index as u32);
        }
        let mut page_map = HashMap::with_capacity(pages.page_count());
        for page in &pages.pages {
            page_map.insert(
                page.id.0,
                PageResidency {
                    page_id: page.id,
                    state: PageResidencyState::Absent,
                    slot: None,
                    slot_generation: 0,
                    attribute_lod: AttributeLod::Degree0,
                    last_visible_frame: 0,
                },
            );
        }
        Self {
            scene_revision: 1,
            budgets,
            pages: page_map,
            slots,
            free_slots,
            frame_index: 0,
        }
    }

    pub fn scene_revision(&self) -> u64 {
        self.scene_revision
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }

    pub fn advance_frame(&mut self) {
        self.frame_index = self.frame_index.saturating_add(1);
    }

    pub fn bump_scene_revision(&mut self) {
        self.scene_revision = self.scene_revision.saturating_add(1);
        for page in self.pages.values_mut() {
            page.state = PageResidencyState::Absent;
            page.slot = None;
            page.slot_generation = 0;
            page.attribute_lod = AttributeLod::Degree0;
            page.last_visible_frame = 0;
        }
        for slot in &mut self.slots {
            if slot.page_id.is_some() {
                slot.generation = slot.generation.saturating_add(1);
                slot.page_id = None;
            }
        }
        self.free_slots.clear();
        for slot in &self.slots {
            self.free_slots.push_back(slot.index);
        }
    }

    pub fn page(&self, page_id: PageId) -> Option<&PageResidency> {
        self.pages.get(&page_id.0)
    }

    pub fn resident_page_ids(&self) -> Vec<PageId> {
        self.pages
            .values()
            .filter(|page| page.state == PageResidencyState::Resident)
            .map(|page| page.page_id)
            .collect()
    }

    pub fn resident_tokens(&self) -> Vec<AsyncPageToken> {
        let mut tokens: Vec<_> = self
            .pages
            .values()
            .filter(|page| page.state == PageResidencyState::Resident)
            .filter_map(|page| {
                Some(AsyncPageToken {
                    scene_revision: self.scene_revision,
                    page_id: page.page_id,
                    slot: page.slot?,
                    slot_generation: page.slot_generation,
                })
            })
            .collect();
        tokens.sort_by_key(|token| token.slot);
        tokens
    }

    pub fn inflight_count(&self) -> usize {
        self.pages
            .values()
            .filter(|page| {
                matches!(
                    page.state,
                    PageResidencyState::Requested
                        | PageResidencyState::CompressedReady
                        | PageResidencyState::DecodedReady
                        | PageResidencyState::Uploading
                        | PageResidencyState::Evicting
                )
            })
            .count()
    }

    pub fn resident_count(&self) -> usize {
        self.pages
            .values()
            .filter(|page| page.state == PageResidencyState::Resident)
            .count()
    }

    pub fn mark_visible(&mut self, page_id: PageId) -> Result<(), ResidencyError> {
        let page = self
            .pages
            .get_mut(&page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        page.last_visible_frame = self.frame_index;
        Ok(())
    }

    pub fn request_page(&mut self, page_id: PageId) -> Result<AsyncPageToken, ResidencyError> {
        if self.inflight_count() >= self.budgets.max_inflight_pages {
            return Err(ResidencyError::BudgetExceeded);
        }
        let page = self
            .pages
            .get_mut(&page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        if page.state != PageResidencyState::Absent {
            return Err(ResidencyError::InvalidTransition {
                from: page.state,
                to: PageResidencyState::Requested,
            });
        }
        let slot = self
            .free_slots
            .pop_front()
            .ok_or(ResidencyError::NoFreeSlot)?;
        let slot_entry = &mut self.slots[slot as usize];
        slot_entry.page_id = Some(page_id);
        page.state = PageResidencyState::Requested;
        page.slot = Some(slot);
        page.slot_generation = slot_entry.generation;
        Ok(AsyncPageToken {
            scene_revision: self.scene_revision,
            page_id,
            slot,
            slot_generation: slot_entry.generation,
        })
    }

    pub fn advance_to_compressed_ready(
        &mut self,
        token: AsyncPageToken,
    ) -> Result<(), ResidencyError> {
        self.validate_token(token)?;
        self.transition(token.page_id, PageResidencyState::CompressedReady)
    }

    pub fn advance_to_decoded_ready(
        &mut self,
        token: AsyncPageToken,
    ) -> Result<(), ResidencyError> {
        self.validate_token(token)?;
        self.transition(token.page_id, PageResidencyState::DecodedReady)
    }

    pub fn advance_to_uploading(&mut self, token: AsyncPageToken) -> Result<(), ResidencyError> {
        self.validate_token(token)?;
        self.transition(token.page_id, PageResidencyState::Uploading)
    }

    pub fn complete_resident(&mut self, token: AsyncPageToken) -> Result<(), ResidencyError> {
        self.validate_token(token)?;
        self.transition(token.page_id, PageResidencyState::Resident)
    }

    pub fn cancel_inflight(&mut self, token: AsyncPageToken) -> Result<(), ResidencyError> {
        self.validate_token(token)?;
        let page = self
            .pages
            .get_mut(&token.page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        if !matches!(
            page.state,
            PageResidencyState::Requested
                | PageResidencyState::CompressedReady
                | PageResidencyState::DecodedReady
                | PageResidencyState::Uploading
        ) {
            return Err(ResidencyError::InvalidTransition {
                from: page.state,
                to: PageResidencyState::Absent,
            });
        }
        page.state = PageResidencyState::Absent;
        page.slot = None;
        page.slot_generation = 0;
        page.attribute_lod = AttributeLod::Degree0;

        let slot = &mut self.slots[token.slot as usize];
        if slot.generation != token.slot_generation || slot.page_id != Some(token.page_id) {
            return Err(ResidencyError::StaleAsyncResult);
        }
        slot.page_id = None;
        slot.generation = slot.generation.saturating_add(1);
        self.free_slots.push_back(token.slot);
        Ok(())
    }

    pub fn begin_evict(&mut self, page_id: PageId) -> Result<AsyncPageToken, ResidencyError> {
        let page = self
            .pages
            .get(&page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        if page.state != PageResidencyState::Resident {
            return Err(ResidencyError::InvalidTransition {
                from: page.state,
                to: PageResidencyState::Evicting,
            });
        }
        let slot = page.slot.ok_or(ResidencyError::SlotMismatch)?;
        let generation = page.slot_generation;
        self.transition(page_id, PageResidencyState::Evicting)?;
        Ok(AsyncPageToken {
            scene_revision: self.scene_revision,
            page_id,
            slot,
            slot_generation: generation,
        })
    }

    pub fn finish_evict(&mut self, token: AsyncPageToken) -> Result<(), ResidencyError> {
        self.validate_token(token)?;
        let page = self
            .pages
            .get_mut(&token.page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        if page.state != PageResidencyState::Evicting {
            return Err(ResidencyError::InvalidTransition {
                from: page.state,
                to: PageResidencyState::Absent,
            });
        }
        page.state = PageResidencyState::Absent;
        page.slot = None;
        page.slot_generation = 0;
        page.attribute_lod = AttributeLod::Degree0;

        let slot = &mut self.slots[token.slot as usize];
        if slot.generation != token.slot_generation || slot.page_id != Some(token.page_id) {
            return Err(ResidencyError::StaleAsyncResult);
        }
        slot.page_id = None;
        slot.generation = slot.generation.saturating_add(1);
        self.free_slots.push_back(token.slot);
        Ok(())
    }

    pub fn set_attribute_lod(
        &mut self,
        page_id: PageId,
        lod: AttributeLod,
    ) -> Result<(), ResidencyError> {
        let page = self
            .pages
            .get_mut(&page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        if page.state != PageResidencyState::Resident {
            return Err(ResidencyError::InvalidTransition {
                from: page.state,
                to: page.state,
            });
        }
        page.attribute_lod = lod;
        Ok(())
    }

    pub fn validate_token(&self, token: AsyncPageToken) -> Result<(), ResidencyError> {
        if token.scene_revision != self.scene_revision {
            return Err(ResidencyError::StaleAsyncResult);
        }
        let page = self
            .pages
            .get(&token.page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        if page.slot != Some(token.slot) || page.slot_generation != token.slot_generation {
            return Err(ResidencyError::StaleAsyncResult);
        }
        let slot = self
            .slots
            .get(token.slot as usize)
            .ok_or(ResidencyError::SlotMismatch)?;
        if slot.generation != token.slot_generation || slot.page_id != Some(token.page_id) {
            return Err(ResidencyError::StaleAsyncResult);
        }
        Ok(())
    }

    fn transition(
        &mut self,
        page_id: PageId,
        to: PageResidencyState,
    ) -> Result<(), ResidencyError> {
        let page = self
            .pages
            .get_mut(&page_id.0)
            .ok_or(ResidencyError::UnknownPage)?;
        let from = page.state;
        let allowed = matches!(
            (from, to),
            (PageResidencyState::Absent, PageResidencyState::Requested)
                | (
                    PageResidencyState::Requested,
                    PageResidencyState::CompressedReady
                )
                | (
                    PageResidencyState::CompressedReady,
                    PageResidencyState::DecodedReady
                )
                | (
                    PageResidencyState::DecodedReady,
                    PageResidencyState::Uploading
                )
                | (PageResidencyState::Uploading, PageResidencyState::Resident)
                | (PageResidencyState::Resident, PageResidencyState::Evicting)
                | (PageResidencyState::Evicting, PageResidencyState::Absent)
        );
        if !allowed {
            return Err(ResidencyError::InvalidTransition { from, to });
        }
        page.state = to;
        Ok(())
    }

    /// Evict the least-recently-visible resident page, if any.
    pub fn evict_lru_resident(&mut self) -> Result<Option<AsyncPageToken>, ResidencyError> {
        let candidate = self
            .pages
            .values()
            .filter(|page| page.state == PageResidencyState::Resident)
            .min_by_key(|page| page.last_visible_frame)
            .map(|page| page.page_id);
        match candidate {
            Some(page_id) => Ok(Some(self.begin_evict(page_id)?)),
            None => Ok(None),
        }
    }

    pub fn resident_attribute_mix(&self) -> HashMap<AttributeLod, usize> {
        let mut mix = HashMap::new();
        for page in self.pages.values() {
            if page.state == PageResidencyState::Resident {
                *mix.entry(page.attribute_lod).or_insert(0) += 1;
            }
        }
        mix
    }

    pub fn known_page_ids(&self) -> HashSet<PageId> {
        self.pages.values().map(|page| page.page_id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial_pages::partition_scene_pages;
    use gsplat_core::{SceneBuffers, Vec3f};

    fn sample_pages(count: usize) -> SpatialPageSet {
        let scene = SceneBuffers {
            positions: (0..count).map(|i| Vec3f::new(i as f32, 0.0, 1.0)).collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0, 0.0, 0.0]; count],
            sh_degree: 0,
            sh_rest: None,
        };
        partition_scene_pages(&scene, 2, 4)
    }

    fn drive_to_resident(
        manager: &mut ResidencyManager,
        page_id: PageId,
    ) -> Result<AsyncPageToken, ResidencyError> {
        let token = manager.request_page(page_id)?;
        manager.advance_to_compressed_ready(token)?;
        manager.advance_to_decoded_ready(token)?;
        manager.advance_to_uploading(token)?;
        manager.complete_resident(token)?;
        Ok(token)
    }

    fn manager(
        pages: &SpatialPageSet,
        max_resident_pages: usize,
        max_inflight_pages: usize,
    ) -> ResidencyManager {
        ResidencyManager::new(
            pages,
            ResidencyBudgets {
                max_resident_pages,
                max_inflight_pages,
            },
        )
    }

    #[test]
    fn happy_path_reaches_resident_and_rejects_stale_generation() {
        let pages = sample_pages(8);
        let mut manager = manager(&pages, 4, 4);
        let page_id = pages.pages[0].id;
        let token = drive_to_resident(&mut manager, page_id).unwrap();
        assert_eq!(
            manager.page(page_id).unwrap().state,
            PageResidencyState::Resident
        );

        let evict = manager.begin_evict(page_id).unwrap();
        manager.finish_evict(evict).unwrap();
        assert_eq!(
            manager.page(page_id).unwrap().state,
            PageResidencyState::Absent
        );

        // Old token must not revive the slot after generation bump.
        assert_eq!(
            manager.complete_resident(token),
            Err(ResidencyError::StaleAsyncResult)
        );
    }

    #[test]
    fn scene_revision_invalidates_inflight_tokens() {
        let pages = sample_pages(6);
        let mut manager = manager(&pages, 4, 4);
        let token = manager.request_page(pages.pages[0].id).unwrap();
        manager.bump_scene_revision();
        assert_eq!(
            manager.advance_to_compressed_ready(token),
            Err(ResidencyError::StaleAsyncResult)
        );
    }

    #[test]
    fn cancel_invalidates_inflight_token_and_releases_slot_generation() {
        let pages = sample_pages(6);
        let mut manager = manager(&pages, 1, 1);
        let cancelled = manager.request_page(pages.pages[0].id).unwrap();
        manager.advance_to_compressed_ready(cancelled).unwrap();
        manager.cancel_inflight(cancelled).unwrap();
        assert_eq!(
            manager.advance_to_decoded_ready(cancelled),
            Err(ResidencyError::StaleAsyncResult)
        );
        let replacement = manager.request_page(pages.pages[1].id).unwrap();
        assert_eq!(replacement.slot, cancelled.slot);
        assert!(replacement.slot_generation > cancelled.slot_generation);
    }

    #[test]
    fn inflight_budget_is_enforced() {
        let pages = sample_pages(10);
        let mut manager = manager(&pages, 8, 1);
        manager.request_page(pages.pages[0].id).unwrap();
        assert_eq!(
            manager.request_page(pages.pages[1].id),
            Err(ResidencyError::BudgetExceeded)
        );
    }

    #[test]
    fn lru_eviction_prefers_older_visibility() {
        let pages = sample_pages(8);
        let mut manager = manager(&pages, 2, 2);
        let a = pages.pages[0].id;
        let b = pages.pages[1].id;
        drive_to_resident(&mut manager, a).unwrap();
        manager.mark_visible(a).unwrap();
        manager.advance_frame();
        drive_to_resident(&mut manager, b).unwrap();
        manager.mark_visible(b).unwrap();
        manager.advance_frame();

        let evict = manager.evict_lru_resident().unwrap().unwrap();
        assert_eq!(evict.page_id, a);
        manager.finish_evict(evict).unwrap();
        assert_eq!(manager.resident_count(), 1);
        assert_eq!(manager.page(b).unwrap().state, PageResidencyState::Resident);
    }

    #[test]
    fn attribute_lod_only_on_resident_pages() {
        let pages = sample_pages(4);
        let mut manager = manager(&pages, 2, 2);
        let page_id = pages.pages[0].id;
        assert!(
            manager
                .set_attribute_lod(page_id, AttributeLod::Degree3)
                .is_err()
        );
        drive_to_resident(&mut manager, page_id).unwrap();
        manager
            .set_attribute_lod(page_id, AttributeLod::Degree2)
            .unwrap();
        let mix = manager.resident_attribute_mix();
        assert_eq!(mix.get(&AttributeLod::Degree2).copied(), Some(1));
    }
}
