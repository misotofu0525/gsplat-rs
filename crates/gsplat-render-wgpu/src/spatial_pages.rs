//! Spatial page metadata for the Phase D paged active-atlas path.
//!
//! Pages are built by assigning splats to a uniform grid over the scene AABB,
//! then packing each non-empty cell into one or more pages of at most
//! `page_capacity` splat indices. This is CPU metadata only; GPU atlas upload
//! remains a later Phase D slice.

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
    /// Grid cell this page was packed from (for coarse coverage tests).
    pub grid_cell: u32,
}

impl SpatialPage {
    pub fn splat_count(&self) -> usize {
        self.splat_indices.len()
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

    let mut pages = Vec::new();
    let mut next_page_id = 0_u32;
    for (cell, indices) in cell_indices.into_iter().enumerate() {
        if indices.is_empty() {
            continue;
        }
        for chunk in indices.chunks(page_capacity) {
            let splat_indices = chunk.to_vec();
            let bounds = PageBounds::from_positions(&scene.positions, &splat_indices);
            pages.push(SpatialPage {
                id: PageId(next_page_id),
                bounds,
                splat_indices,
                grid_cell: cell as u32,
            });
            next_page_id = next_page_id.saturating_add(1);
        }
    }

    SpatialPageSet {
        scene_bounds,
        page_capacity,
        grid_axis,
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
}
