//! CPU page selection for Phase D.
//!
//! Fills residency budgets in design order:
//! 1. preserve a coarse covering set;
//! 2. refine visible pages by projected contribution (distance proxy);
//! 3. request uploads for newly exposed pages;
//! 4. evict LRU pages only when over budget and outside the retain set.

use gsplat_core::Vec3f;

use crate::residency::{PageResidencyState, ResidencyError, ResidencyManager};
use crate::spatial_pages::{PageId, SpatialPageSet};

/// Camera sample used by the CPU selector (position only for the first slice).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SchedulerView {
    pub position: Vec3f,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SchedulerConfig {
    /// Maximum pages that should remain resident after a schedule tick.
    pub target_resident_pages: usize,
    /// Distance used to mark pages as part of the coarse covering set.
    pub coarse_cover_radius: f32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            target_resident_pages: 8,
            coarse_cover_radius: 2.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleOutcome {
    pub requested: Vec<PageId>,
    pub evicted: Vec<PageId>,
    pub retained: Vec<PageId>,
    pub visible: Vec<PageId>,
}

fn oldest_unprotected_resident(manager: &ResidencyManager, retain: &[PageId]) -> Option<PageId> {
    manager
        .resident_page_ids()
        .into_iter()
        .filter(|page_id| !retain.contains(page_id))
        .filter_map(|page_id| {
            manager
                .page(page_id)
                .map(|page| ((page.last_visible_frame, page_id.0), page_id))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, page_id)| page_id)
}

/// Rank pages by distance to the view and drive residency requests/evictions.
pub fn schedule_pages(
    pages: &SpatialPageSet,
    manager: &mut ResidencyManager,
    view: SchedulerView,
    config: SchedulerConfig,
) -> Result<ScheduleOutcome, ResidencyError> {
    manager.advance_frame();

    let mut ranked: Vec<(PageId, f32)> = pages
        .pages
        .iter()
        .map(|page| {
            let center = page.bounds.center();
            let dx = center.x - view.position.x;
            let dy = center.y - view.position.y;
            let dz = center.z - view.position.z;
            (page.id, dx * dx + dy * dy + dz * dz)
        })
        .collect();
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let cover_radius2 = config.coarse_cover_radius * config.coarse_cover_radius;
    let mut visible = Vec::new();
    for &(page_id, distance2) in &ranked {
        if distance2 <= cover_radius2 {
            visible.push(page_id);
        }
    }
    let target = config.target_resident_pages.max(1);
    let pinned: Vec<PageId> = pages
        .pages
        .iter()
        .filter(|page| page.is_coarse_cover())
        .take(target)
        .map(|page| page.id)
        .collect();
    let refinement_target = target.saturating_sub(pinned.len());
    let ranked_refinements: Vec<(PageId, f32)> = ranked
        .iter()
        .copied()
        .filter(|(page_id, _)| !pinned.contains(page_id))
        .collect();
    // Pin explicit coarse coverage, then keep current refinement pages while
    // they remain close to the target cutoff. The squared cover radius acts as
    // a deterministic retention margin for small camera motion.
    let cutoff = ranked_refinements
        .get(
            refinement_target
                .saturating_sub(1)
                .min(ranked_refinements.len().saturating_sub(1)),
        )
        .map(|&(_, distance2)| distance2)
        .unwrap_or(0.0);
    let residents = manager.resident_page_ids();
    let mut retain = pinned;
    retain.extend(
        ranked_refinements
            .iter()
            .filter(|&&(page_id, distance2)| {
                residents.contains(&page_id) && distance2 <= cutoff + cover_radius2
            })
            .take(refinement_target)
            .map(|&(page_id, _)| page_id),
    );
    for &(page_id, _) in &ranked_refinements {
        if retain.len() >= target {
            break;
        }
        if !retain.contains(&page_id) {
            retain.push(page_id);
        }
    }
    for &page_id in &retain {
        manager.mark_visible(page_id)?;
    }
    if visible.is_empty()
        && let Some(&page_id) = retain.first()
    {
        visible.push(page_id);
    }

    let mut requested = Vec::new();
    let mut evicted = Vec::new();
    for &page_id in &retain {
        let state = manager
            .page(page_id)
            .ok_or(ResidencyError::UnknownPage)?
            .state;
        if state == PageResidencyState::Resident {
            continue;
        }
        if state != PageResidencyState::Absent {
            return Err(ResidencyError::InvalidTransition {
                from: state,
                to: PageResidencyState::Resident,
            });
        }

        let token = match manager.request_page(page_id) {
            Ok(token) => token,
            Err(ResidencyError::BudgetExceeded) => {
                return Err(ResidencyError::BudgetExceeded);
            }
            Err(ResidencyError::NoFreeSlot) => {
                let victim = oldest_unprotected_resident(manager, &retain)
                    .ok_or(ResidencyError::NoFreeSlot)?;
                let evict_token = manager.begin_evict(victim)?;
                manager.finish_evict(evict_token)?;
                evicted.push(victim);
                manager.request_page(page_id)?
            }
            Err(error) => return Err(error),
        };
        // This selector owns a synchronous CPU-side request pipeline. Any
        // externally inflight retained page is reported as an error above
        // instead of being misreported as resident.
        manager.advance_to_compressed_ready(token)?;
        manager.advance_to_decoded_ready(token)?;
        manager.advance_to_uploading(token)?;
        manager.complete_resident(token)?;
        requested.push(page_id);
    }

    while manager.resident_count() > target {
        let page_id =
            oldest_unprotected_resident(manager, &retain).ok_or(ResidencyError::NoFreeSlot)?;
        let token = manager.begin_evict(page_id)?;
        manager.finish_evict(token)?;
        evicted.push(page_id);
    }

    Ok(ScheduleOutcome {
        requested,
        evicted,
        retained: retain,
        visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::residency::{
        AsyncPageToken, PageResidencyState, ResidencyBudgets, ResidencyManager,
    };
    use crate::spatial_pages::{
        PageBounds, partition_scene_pages, partition_scene_pages_with_coarse_cover,
    };
    use gsplat_core::SceneBuffers;

    fn line_scene(count: usize) -> SpatialPageSet {
        let scene = SceneBuffers {
            positions: (0..count)
                .map(|i| Vec3f::new(i as f32 * 3.0, 0.0, 0.0))
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0, 0.0, 0.0]; count],
            sh_degree: 0,
            sh_rest: None,
        };
        partition_scene_pages(&scene, 1, count.max(1))
    }

    fn drive_to_resident(manager: &mut ResidencyManager, page_id: PageId) -> AsyncPageToken {
        let token = manager.request_page(page_id).unwrap();
        manager.advance_to_compressed_ready(token).unwrap();
        manager.advance_to_decoded_ready(token).unwrap();
        manager.advance_to_uploading(token).unwrap();
        manager.complete_resident(token).unwrap();
        token
    }

    fn coarse_pressure_pages() -> SpatialPageSet {
        let scene = SceneBuffers {
            positions: (0..7)
                .map(|index| Vec3f::new(index as f32, 0.0, 1.0))
                .collect(),
            opacity: vec![0.0; 7],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; 7],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 7],
            color_dc: vec![[0.0, 0.0, 0.0]; 7],
            sh_degree: 0,
            sh_rest: None,
        };
        partition_scene_pages_with_coarse_cover(&scene, 1, 7, 4)
    }

    fn set_page_x(pages: &mut SpatialPageSet, page_id: PageId, x: f32) {
        pages.page_mut(page_id).unwrap().bounds = PageBounds {
            min: [x, 0.0, 0.0],
            max: [x + 0.01, 0.01, 0.01],
        };
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

    fn schedule_at(
        pages: &SpatialPageSet,
        manager: &mut ResidencyManager,
        position: Vec3f,
        target_resident_pages: usize,
        coarse_cover_radius: f32,
    ) -> Result<ScheduleOutcome, ResidencyError> {
        schedule_pages(
            pages,
            manager,
            SchedulerView { position },
            SchedulerConfig {
                target_resident_pages,
                coarse_cover_radius,
            },
        )
    }

    #[test]
    fn schedule_requests_near_pages_and_keeps_coarse_cover() {
        let pages = line_scene(6);
        let mut manager = manager(&pages, 3, 3);
        let outcome = schedule_at(&pages, &mut manager, Vec3f::new(0.0, 0.0, 0.0), 2, 4.0).unwrap();
        assert!(!outcome.retained.is_empty());
        assert!(manager.resident_count() <= 3);
        assert!(manager.resident_count() >= 1);
        for page_id in &outcome.retained {
            assert_eq!(
                manager.page(*page_id).unwrap().state,
                PageResidencyState::Resident
            );
        }
    }

    #[test]
    fn camera_jump_replaces_far_residents_with_new_cover() {
        let pages = line_scene(8);
        let mut manager = manager(&pages, 2, 2);
        let first = schedule_at(&pages, &mut manager, Vec3f::new(0.0, 0.0, 0.0), 2, 3.5).unwrap();
        assert!(!first.retained.is_empty());

        let jumped = schedule_at(&pages, &mut manager, Vec3f::new(21.0, 0.0, 0.0), 2, 3.5).unwrap();
        assert!(!jumped.retained.is_empty());
        // New cover should include a page near the jumped camera.
        let near_jump = pages
            .pages
            .iter()
            .min_by(|a, b| {
                let da = {
                    let c = a.bounds.center();
                    (c.x - 21.0).abs()
                };
                let db = {
                    let c = b.bounds.center();
                    (c.x - 21.0).abs()
                };
                da.partial_cmp(&db).unwrap()
            })
            .unwrap()
            .id;
        assert!(
            jumped.retained.contains(&near_jump)
                || manager.page(near_jump).unwrap().state == PageResidencyState::Resident,
            "camera jump must residentially cover a near page"
        );
        assert!(manager.resident_count() <= 2);
    }

    #[test]
    fn fixed_four_slot_schedule_fills_sparse_grid_working_set() {
        let grid_axis = 8usize;
        let mut positions: Vec<_> = (0..grid_axis)
            .flat_map(|z| {
                (0..grid_axis).flat_map(move |y| {
                    (0..grid_axis).map(move |x| Vec3f::new(x as f32, y as f32, z as f32 + 1.0))
                })
            })
            .collect();
        positions.extend_from_within(..128);
        let count = positions.len();
        let scene = SceneBuffers {
            positions,
            opacity: vec![0.0; count],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0, 0.0, 0.0]; count],
            sh_degree: 0,
            sh_rest: None,
        };
        let pages = partition_scene_pages(&scene, 128, grid_axis);
        assert_eq!(pages.page_count(), 5);

        let mut manager = manager(&pages, 4, 4);
        let outcome =
            schedule_at(&pages, &mut manager, Vec3f::new(3.5, 3.5, -6.0), 4, 2.0).unwrap();

        assert_eq!(outcome.retained.len(), 4);
        assert_eq!(manager.resident_count(), 4);
        let selected: Vec<_> = outcome
            .retained
            .iter()
            .flat_map(|&page_id| pages.page(page_id).unwrap().splat_indices.iter().copied())
            .collect();
        assert_eq!(selected.len(), 4 * 128);
        let unique: std::collections::HashSet<_> = selected.iter().copied().collect();
        assert_eq!(
            unique.len(),
            selected.len(),
            "resident pages must not overlap"
        );
    }

    #[test]
    fn coarse_cover_is_pinned_even_when_refinements_are_nearer() {
        let scene = SceneBuffers {
            positions: (0..640)
                .map(|index| {
                    let cell = index % 512;
                    Vec3f::new(
                        (cell % 8) as f32,
                        ((cell / 8) % 8) as f32,
                        (cell / 64) as f32 + 1.0,
                    )
                })
                .collect(),
            opacity: vec![0.0; 640],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; 640],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 640],
            color_dc: vec![[0.0, 0.0, 0.0]; 640],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut pages = partition_scene_pages_with_coarse_cover(&scene, 128, 8, 4);
        let cover_id = pages
            .pages
            .iter()
            .find(|page| page.is_coarse_cover())
            .unwrap()
            .id;
        pages.page_mut(cover_id).unwrap().bounds = PageBounds {
            min: [100.0, 100.0, 100.0],
            max: [101.0, 101.0, 101.0],
        };
        let mut manager = manager(&pages, 4, 4);
        let outcome = schedule_at(&pages, &mut manager, Vec3f::new(0.0, 0.0, 0.0), 4, 2.0).unwrap();

        assert!(outcome.retained.contains(&cover_id));
        assert_eq!(manager.resident_count(), 4);
        assert_eq!(
            manager.page(cover_id).unwrap().state,
            PageResidencyState::Resident
        );
    }

    #[test]
    fn inflight_pressure_does_not_evict_the_pinned_working_set() {
        let mut pages = coarse_pressure_pages();
        for (page_id, x) in [
            (PageId(0), 1_000.0),
            (PageId(1), 0.0),
            (PageId(2), 1.0),
            (PageId(3), 2.0),
            (PageId(4), 100.0),
            (PageId(5), 200.0),
            (PageId(6), 300.0),
        ] {
            set_page_x(&mut pages, page_id, x);
        }
        let mut manager = manager(&pages, 4, 1);
        let cover_token = drive_to_resident(&mut manager, PageId(0));
        drive_to_resident(&mut manager, PageId(1));
        let pending = manager.request_page(PageId(6)).unwrap();
        let mut residents_before = manager.resident_page_ids();
        residents_before.sort_by_key(|page_id| page_id.0);

        let result = schedule_at(&pages, &mut manager, Vec3f::new(0.0, 0.0, 0.0), 4, 0.0);

        assert_eq!(result, Err(ResidencyError::BudgetExceeded));
        let mut residents_after = manager.resident_page_ids();
        residents_after.sort_by_key(|page_id| page_id.0);
        assert_eq!(residents_after, residents_before);
        assert_eq!(manager.resident_count(), 2);
        assert_eq!(manager.inflight_count(), 1);
        assert_eq!(manager.validate_token(pending), Ok(()));
        let cover = manager.page(PageId(0)).unwrap();
        assert_eq!(cover.state, PageResidencyState::Resident);
        assert_eq!(cover.slot, Some(cover_token.slot));
        assert_eq!(cover.slot_generation, cover_token.slot_generation);
    }

    #[test]
    fn full_slot_camera_jump_preserves_cover_and_reports_refinement_evictions() {
        let mut pages = coarse_pressure_pages();
        for (page_id, x) in [
            (PageId(0), 1_000.0),
            (PageId(1), -102.0),
            (PageId(2), -101.0),
            (PageId(3), -100.0),
            (PageId(4), 100.0),
            (PageId(5), 101.0),
            (PageId(6), 102.0),
        ] {
            set_page_x(&mut pages, page_id, x);
        }
        let mut manager = manager(&pages, 4, 4);

        let first =
            schedule_at(&pages, &mut manager, Vec3f::new(-100.0, 0.0, 0.0), 4, 0.0).unwrap();
        let cover_before = manager
            .resident_tokens()
            .into_iter()
            .find(|token| token.page_id == PageId(0))
            .unwrap();

        let jumped =
            schedule_at(&pages, &mut manager, Vec3f::new(100.0, 0.0, 0.0), 4, 0.0).unwrap();

        let as_set =
            |ids: &[PageId]| -> std::collections::HashSet<_> { ids.iter().copied().collect() };
        assert_eq!(
            as_set(&first.retained),
            as_set(&[PageId(0), PageId(1), PageId(2), PageId(3)])
        );
        assert_eq!(
            as_set(&jumped.retained),
            as_set(&[PageId(0), PageId(4), PageId(5), PageId(6)])
        );
        assert_eq!(
            as_set(&jumped.requested),
            as_set(&[PageId(4), PageId(5), PageId(6)])
        );
        assert_eq!(jumped.evicted.len(), 3);
        assert_eq!(
            as_set(&jumped.evicted),
            as_set(&[PageId(1), PageId(2), PageId(3)])
        );
        assert!(!jumped.evicted.contains(&PageId(0)));
        assert_eq!(manager.resident_count(), 4);
        assert_eq!(
            as_set(&manager.resident_page_ids()),
            as_set(&jumped.retained)
        );
        assert!(jumped.retained.iter().all(|page_id| {
            manager.page(*page_id).unwrap().state == PageResidencyState::Resident
        }));
        let cover_after = manager
            .resident_tokens()
            .into_iter()
            .find(|token| token.page_id == PageId(0))
            .unwrap();
        assert_eq!(cover_after.slot, cover_before.slot);
        assert_eq!(cover_after.slot_generation, cover_before.slot_generation);
    }
}
