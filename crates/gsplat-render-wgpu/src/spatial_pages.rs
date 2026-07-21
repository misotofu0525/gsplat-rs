//! Local spatial metadata built from a full scene for the paged prototype.

// The frozen diagnostic retains a few analysis helpers used only by tests.
// Keep them internal without exposing them as a compatibility surface.
#![allow(dead_code)]

use gsplat_core::{SceneBuffers, Vec3f};

use crate::packed_atlas::SceneBounds;

/// Default prototype page capacity from the competitive design (256×256 tile).
pub const DEFAULT_PAGE_CAPACITY: usize = 65_536;

/// Stable identity for a spatial page within one scene revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u32);

/// Axis-aligned page bounds in world/RUF space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl PageBounds {
    pub fn from_positions(positions: &[Vec3f], indices: &[u32]) -> Self {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for &index in indices {
            let position = positions[index as usize];
            min[0] = min[0].min(position.x);
            min[1] = min[1].min(position.y);
            min[2] = min[2].min(position.z);
            max[0] = max[0].max(position.x);
            max[1] = max[1].max(position.y);
            max[2] = max[2].max(position.z);
        }
        for axis in 0..3 {
            if !min[axis].is_finite() || !max[axis].is_finite() {
                min[axis] = 0.0;
                max[axis] = 1.0;
            } else if (max[axis] - min[axis]).abs() < 1e-6 {
                max[axis] = min[axis] + 1e-3;
            }
        }
        Self { min, max }
    }

    pub fn center(&self) -> Vec3f {
        Vec3f::new(
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        )
    }

    pub fn contains_point(&self, point: Vec3f) -> bool {
        point.x >= self.min[0]
            && point.x <= self.max[0]
            && point.y >= self.min[1]
            && point.y <= self.max[1]
            && point.z >= self.min[2]
            && point.z <= self.max[2]
    }
}

/// One spatial page of splat indices plus bounds metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialPage {
    pub id: PageId,
    pub bounds: PageBounds,
    /// Indices into the source `SceneBuffers`.
    pub splat_indices: Vec<u32>,
    /// First grid cell in this page's stable spatial packing order.
    /// `u32::MAX` is reserved internally for the global coarse-cover page.
    pub grid_cell: u32,
}

impl SpatialPage {
    pub fn splat_count(&self) -> usize {
        self.splat_indices.len()
    }

    pub(crate) fn is_coarse_cover(&self) -> bool {
        self.grid_cell == u32::MAX
    }
}

/// Complete spatial page set for one loaded scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialPageSet {
    pub scene_bounds: SceneBounds,
    pub page_capacity: usize,
    pub grid_axis: usize,
    pub pages: Vec<SpatialPage>,
}

impl SpatialPageSet {
    pub fn page(&self, id: PageId) -> Option<&SpatialPage> {
        self.pages.iter().find(|page| page.id == id)
    }

    #[cfg(test)]
    pub fn page_mut(&mut self, id: PageId) -> Option<&mut SpatialPage> {
        self.pages.iter_mut().find(|page| page.id == id)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn total_splats(&self) -> usize {
        self.pages.iter().map(SpatialPage::splat_count).sum()
    }
}

/// Build spatial pages for `scene`.
///
/// `page_capacity` caps splats per page. `grid_axis` chooses the uniform grid
/// resolution along each axis (at least 1). Empty cells are skipped.
pub fn partition_scene_pages(
    scene: &SceneBuffers,
    page_capacity: usize,
    grid_axis: usize,
) -> SpatialPageSet {
    let page_capacity = page_capacity.max(1);
    let grid_axis = grid_axis.max(1);
    let scene_bounds = SceneBounds::from_positions(&scene.positions);
    let cell_count = grid_axis
        .saturating_mul(grid_axis)
        .saturating_mul(grid_axis)
        .max(1);

    let mut cell_indices: Vec<Vec<u32>> = vec![Vec::new(); cell_count];
    for (index, position) in scene.positions.iter().enumerate() {
        let cell = grid_cell_index(*position, &scene_bounds, grid_axis);
        cell_indices[cell].push(index as u32);
    }

    // Preserve deterministic cell order for spatial locality, but pack across
    // cell boundaries. Emitting one page per sparse cell leaves almost every
    // fixed-capacity atlas slot empty on real scenes such as Kitsune.
    let spatial_indices: Vec<u32> = cell_indices.into_iter().flatten().collect();
    let mut pages = Vec::with_capacity(spatial_indices.len().div_ceil(page_capacity));
    for (page_index, chunk) in spatial_indices.chunks(page_capacity).enumerate() {
        let splat_indices = chunk.to_vec();
        let bounds = PageBounds::from_positions(&scene.positions, &splat_indices);
        let grid_cell = chunk
            .first()
            .map(|&index| {
                grid_cell_index(scene.positions[index as usize], &scene_bounds, grid_axis)
            })
            .unwrap_or(0) as u32;
        pages.push(SpatialPage {
            id: PageId(page_index as u32),
            bounds,
            splat_indices,
            grid_cell,
        });
    }

    SpatialPageSet {
        scene_bounds,
        page_capacity,
        grid_axis,
        pages,
    }
}

pub(crate) fn partition_scene_pages_with_coarse_cover(
    scene: &SceneBuffers,
    page_capacity: usize,
    grid_axis: usize,
    resident_slots: usize,
) -> SpatialPageSet {
    let dense = partition_scene_pages(scene, page_capacity, grid_axis);
    if dense.page_count() <= resident_slots.max(1) || dense.pages.is_empty() {
        return dense;
    }

    let spatial_indices: Vec<u32> = dense
        .pages
        .iter()
        .flat_map(|page| page.splat_indices.iter().copied())
        .collect();
    let cover_count = dense.page_capacity.min(spatial_indices.len());
    let occupied_ranges = occupied_cell_ranges(
        scene,
        &dense.scene_bounds,
        dense.grid_axis,
        &spatial_indices,
    );
    let mut cover_positions = vec![false; spatial_indices.len()];
    let mut selected_count = 0usize;
    if occupied_ranges.len() <= cover_count {
        // A fixed global cover must not lose a sparse region merely because a
        // different cell contains most of the scene. Seed every occupied cell
        // when the page budget permits, then fill the rest proportionally.
        for range in &occupied_ranges {
            let position = range.start + range.len() / 2;
            cover_positions[position] = true;
            selected_count += 1;
        }
        for sample in 0..cover_count {
            if selected_count == cover_count {
                break;
            }
            let position = proportional_sample_position(sample, spatial_indices.len(), cover_count);
            if !cover_positions[position] {
                cover_positions[position] = true;
                selected_count += 1;
            }
        }
        if selected_count < cover_count {
            for selected in &mut cover_positions {
                if selected_count == cover_count {
                    break;
                }
                if !*selected {
                    *selected = true;
                    selected_count += 1;
                }
            }
        }
    } else {
        // A page cannot represent more occupied cells than it has entries.
        // Choose cell ranges themselves (including both spatial extremes)
        // rather than letting dense cells monopolize the cover.
        for sample in 0..cover_count {
            let range_index =
                proportional_endpoint_position(sample, occupied_ranges.len(), cover_count);
            let range = &occupied_ranges[range_index];
            cover_positions[range.start + range.len() / 2] = true;
            selected_count += 1;
        }
    }
    debug_assert_eq!(selected_count, cover_count);
    let cover_indices: Vec<u32> = spatial_indices
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(position, index)| cover_positions[position].then_some(index))
        .collect();
    let refinement_indices: Vec<u32> = spatial_indices
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(position, index)| (!cover_positions[position]).then_some(index))
        .collect();

    let mut pages = vec![SpatialPage {
        id: PageId(0),
        bounds: PageBounds::from_positions(&scene.positions, &cover_indices),
        splat_indices: cover_indices,
        grid_cell: u32::MAX,
    }];

    let refinement_page_count = refinement_indices.len().div_ceil(dense.page_capacity);
    let mut offset = 0usize;
    for page_index in 0..refinement_page_count {
        let pages_left = refinement_page_count - page_index;
        let chunk_len = (refinement_indices.len() - offset).div_ceil(pages_left);
        let chunk = &refinement_indices[offset..offset + chunk_len];
        let splat_indices = chunk.to_vec();
        let grid_cell = chunk
            .first()
            .map(|&index| {
                grid_cell_index(
                    scene.positions[index as usize],
                    &dense.scene_bounds,
                    dense.grid_axis,
                )
            })
            .unwrap_or(0) as u32;
        pages.push(SpatialPage {
            id: PageId((page_index + 1) as u32),
            bounds: PageBounds::from_positions(&scene.positions, &splat_indices),
            splat_indices,
            grid_cell,
        });
        offset += chunk_len;
    }

    SpatialPageSet {
        scene_bounds: dense.scene_bounds,
        page_capacity: dense.page_capacity,
        grid_axis: dense.grid_axis,
        pages,
    }
}

fn grid_cell_index(position: Vec3f, bounds: &SceneBounds, grid_axis: usize) -> usize {
    let axis = grid_axis.max(1);
    let extent = [
        (bounds.max[0] - bounds.min[0]).max(1e-6),
        (bounds.max[1] - bounds.min[1]).max(1e-6),
        (bounds.max[2] - bounds.min[2]).max(1e-6),
    ];
    let coords = [
        (((position.x - bounds.min[0]) / extent[0]).clamp(0.0, 0.999_999) * axis as f32) as usize,
        (((position.y - bounds.min[1]) / extent[1]).clamp(0.0, 0.999_999) * axis as f32) as usize,
        (((position.z - bounds.min[2]) / extent[2]).clamp(0.0, 0.999_999) * axis as f32) as usize,
    ];
    coords[0] + coords[1] * axis + coords[2] * axis * axis
}

fn proportional_sample_position(sample: usize, source_len: usize, sample_count: usize) -> usize {
    debug_assert!(sample_count > 0);
    debug_assert!(sample < sample_count);
    // Use a wider intermediate: `sample * source_len` overflows `usize` for
    // ordinary large scenes on wasm32 even though the final quotient fits.
    ((sample as u128 * source_len as u128) / sample_count as u128) as usize
}

fn proportional_endpoint_position(sample: usize, source_len: usize, sample_count: usize) -> usize {
    debug_assert!(source_len >= sample_count);
    debug_assert!(sample < sample_count);
    if sample_count == 1 {
        return source_len / 2;
    }
    ((sample as u128 * (source_len - 1) as u128) / (sample_count - 1) as u128) as usize
}

fn occupied_cell_ranges(
    scene: &SceneBuffers,
    bounds: &SceneBounds,
    grid_axis: usize,
    spatial_indices: &[u32],
) -> Vec<std::ops::Range<usize>> {
    let Some(&first_index) = spatial_indices.first() else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut range_start = 0usize;
    let mut current_cell =
        grid_cell_index(scene.positions[first_index as usize], bounds, grid_axis);
    for (position, &index) in spatial_indices.iter().enumerate().skip(1) {
        let cell = grid_cell_index(scene.positions[index as usize], bounds, grid_axis);
        if cell != current_cell {
            ranges.push(range_start..position);
            range_start = position;
            current_cell = cell;
        }
    }
    ranges.push(range_start..spatial_indices.len());
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scene(count: usize) -> SceneBuffers {
        SceneBuffers {
            positions: (0..count)
                .map(|i| {
                    Vec3f::new(
                        (i % 4) as f32 * 0.25,
                        ((i / 4) % 4) as f32 * 0.25,
                        (i / 16) as f32 * 0.5,
                    )
                })
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[-4.0, -4.0, -4.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0, 0.0, 0.0]; count],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn partition_respects_page_capacity_and_covers_all_splats() {
        let scene = sample_scene(20);
        let pages = partition_scene_pages(&scene, 4, 2);
        assert!(pages.page_count() >= 5);
        assert!(pages.pages.iter().all(|page| page.splat_count() <= 4));
        assert_eq!(pages.total_splats(), 20);

        let mut seen = [false; 20];
        for page in &pages.pages {
            for &index in &page.splat_indices {
                assert!(!seen[index as usize], "duplicate splat index {index}");
                seen[index as usize] = true;
            }
        }
        assert!(seen.iter().all(|&value| value));
    }

    #[test]
    fn empty_scene_yields_no_pages() {
        let scene = sample_scene(0);
        let pages = partition_scene_pages(&scene, 8, 2);
        assert_eq!(pages.page_count(), 0);
        assert_eq!(pages.total_splats(), 0);
    }

    #[test]
    fn page_lookup_by_id_works() {
        let scene = sample_scene(8);
        let pages = partition_scene_pages(&scene, 8, 1);
        assert_eq!(pages.page_count(), 1);
        let page = pages.page(PageId(0)).unwrap();
        assert_eq!(page.splat_count(), 8);
        assert!(page.bounds.contains_point(page.bounds.center()));
    }

    #[test]
    fn sparse_grid_cells_are_packed_into_dense_pages() {
        let grid_axis = 8usize;
        let positions: Vec<_> = (0..grid_axis)
            .flat_map(|z| {
                (0..grid_axis).flat_map(move |y| {
                    (0..grid_axis).map(move |x| Vec3f::new(x as f32, y as f32, z as f32 + 1.0))
                })
            })
            .collect();
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

        let pages = partition_scene_pages(&scene, 64, grid_axis);
        assert_eq!(pages.total_splats(), count);
        assert_eq!(
            pages.page_count(),
            8,
            "512 singleton cells should fill eight pages"
        );
        assert!(pages.pages.iter().all(|page| page.splat_count() == 64));
    }

    #[test]
    fn over_slot_scene_has_one_disjoint_global_coarse_cover() {
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

        let pages = partition_scene_pages_with_coarse_cover(&scene, 128, grid_axis, 4);
        assert!(pages.page_count() > 4);
        let covers: Vec<_> = pages
            .pages
            .iter()
            .filter(|page| page.is_coarse_cover())
            .collect();
        assert_eq!(
            covers.len(),
            1,
            "over-slot scenes need one pinned cover page"
        );
        assert!(covers[0].splat_count() <= 128);

        let mut octants = [false; 8];
        for &index in &covers[0].splat_indices {
            let position = scene.positions[index as usize];
            let x = usize::from(position.x >= 3.5);
            let y = usize::from(position.y >= 3.5);
            let z = usize::from(position.z >= 4.5);
            octants[x | (y << 1) | (z << 2)] = true;
        }
        assert!(octants.into_iter().all(|covered| covered));

        let all_indices: Vec<_> = pages
            .pages
            .iter()
            .flat_map(|page| page.splat_indices.iter().copied())
            .collect();
        let unique: std::collections::HashSet<_> = all_indices.iter().copied().collect();
        assert_eq!(all_indices.len(), count);
        assert_eq!(
            unique.len(),
            count,
            "cover and refinement pages must be disjoint"
        );
    }

    #[test]
    fn proportional_cover_sampling_is_unique_at_wasm32_scene_scale() {
        const SOURCE_LEN: usize = 279_199;
        const COVER_COUNT: usize = 65_536;

        let positions: Vec<_> = (0..COVER_COUNT)
            .map(|sample| proportional_sample_position(sample, SOURCE_LEN, COVER_COUNT))
            .collect();
        let unique: std::collections::HashSet<_> = positions.iter().copied().collect();

        assert_eq!(unique.len(), COVER_COUNT);
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            positions
                .last()
                .is_some_and(|&position| position < SOURCE_LEN)
        );

        // Reproduce the former wasm32 arithmetic: saturation starts while the
        // requested cover is still far from complete, collapsing most samples.
        let saturated_unique: std::collections::HashSet<_> = (0..COVER_COUNT as u32)
            .map(|sample| sample.saturating_mul(SOURCE_LEN as u32) / COVER_COUNT as u32)
            .collect();
        assert!(saturated_unique.len() < COVER_COUNT);
    }

    #[test]
    fn coarse_cover_keeps_an_isolated_occupied_cell_under_extreme_density_skew() {
        const DENSE_COUNT: usize = 262_144;
        let mut positions = vec![Vec3f::new(0.0, 0.0, 1.0); DENSE_COUNT];
        positions.push(Vec3f::new(7.0, 7.0, 8.0));
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

        let pages = partition_scene_pages_with_coarse_cover(&scene, 65_536, 8, 4);
        let cover = pages
            .pages
            .iter()
            .find(|page| page.is_coarse_cover())
            .expect("over-slot scene should have a coarse cover");

        assert!(
            cover.splat_indices.contains(&(DENSE_COUNT as u32)),
            "the only splat in the isolated occupied cell must stay in the pinned cover"
        );
    }
}
