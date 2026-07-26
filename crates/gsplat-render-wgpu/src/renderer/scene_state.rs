//! Concrete scene storage and path-derived CPU caches for [`crate::Renderer`].
//!
//! This owner deliberately excludes the Exact prepared runtime, plan policy,
//! semantic generations, terminal evidence, and public statistics. Those
//! remain coordinated and published by the renderer facade.

use gsplat_core::{SceneBuffers, Vec3f};

use crate::GeometryPath;
use crate::cpu::reference::{precompute_alpha_values, precompute_world_covariances};
use crate::data::CameraCovarianceTerms;
use crate::scene::ResidentSceneCpu;
use crate::spatial_pages::{self, DEFAULT_PAGE_CAPACITY, SpatialPageSet};

const DEFAULT_PAGED_ATLAS_SLOTS: usize = 4;

enum SceneSource {
    Empty,
    Wide(SceneBuffers),
    Resident(ResidentSceneCpu),
}

enum DerivedSceneData {
    None,
    Direct {
        world_covariances: Vec<[[f32; 3]; 3]>,
        world_covariance_terms: Vec<CameraCovarianceTerms>,
        alpha_values: Vec<f32>,
    },
    Paged {
        spatial_pages: SpatialPageSet,
    },
}

impl DerivedSceneData {
    fn prepare(path: GeometryPath, scene: Option<&SceneBuffers>) -> Self {
        match (path, scene) {
            (GeometryPath::SortedIndexDirect, Some(scene)) => {
                let world_covariances = precompute_world_covariances(scene);
                let world_covariance_terms = world_covariances
                    .iter()
                    .copied()
                    .map(CameraCovarianceTerms::from_matrix)
                    .collect();
                let alpha_values = precompute_alpha_values(scene);
                Self::Direct {
                    world_covariances,
                    world_covariance_terms,
                    alpha_values,
                }
            }
            (GeometryPath::PagedActiveAtlas, Some(scene)) => Self::Paged {
                spatial_pages: default_spatial_pages(scene),
            },
            _ => Self::None,
        }
    }
}

/// Renderer-owned scene source, derived CPU data, and reusable order scratch.
///
/// Source and derived variants encode the wide/resident and Direct/Paged
/// exclusivity invariants structurally. Replacement prepares a complete new
/// variant before publishing it through the renderer facade.
pub(crate) struct RendererSceneState {
    source: SceneSource,
    derived: DerivedSceneData,
    preprocess_indices: Vec<u32>,
}

pub(crate) struct DirectSceneCpuView<'a> {
    pub(crate) scene: &'a SceneBuffers,
    pub(crate) world_covariances: &'a [[[f32; 3]; 3]],
    pub(crate) world_covariance_terms: &'a [CameraCovarianceTerms],
    pub(crate) alpha_values: &'a [f32],
}

impl RendererSceneState {
    pub(crate) const fn empty() -> Self {
        Self {
            source: SceneSource::Empty,
            derived: DerivedSceneData::None,
            preprocess_indices: Vec::new(),
        }
    }

    pub(crate) fn replace_wide(&mut self, scene: SceneBuffers, path: GeometryPath) {
        let derived = DerivedSceneData::prepare(path, Some(&scene));
        let preprocess_indices = std::mem::take(&mut self.preprocess_indices);
        *self = Self {
            source: SceneSource::Wide(scene),
            derived,
            preprocess_indices,
        };
    }

    pub(crate) fn replace_resident(&mut self, resident: ResidentSceneCpu) {
        let preprocess_indices = std::mem::take(&mut self.preprocess_indices);
        *self = Self {
            source: SceneSource::Resident(resident),
            derived: DerivedSceneData::None,
            preprocess_indices,
        };
    }

    /// Leaves only reusable, empty order storage after Exact runtime publish.
    pub(crate) fn clear_for_exact_runtime(&mut self) {
        let mut preprocess_indices = std::mem::take(&mut self.preprocess_indices);
        preprocess_indices.clear();
        *self = Self {
            source: SceneSource::Empty,
            derived: DerivedSceneData::None,
            preprocess_indices,
        };
    }

    /// Rebuilds only path-derived data; source and order scratch stay intact.
    pub(crate) fn rebuild_for_path(&mut self, path: GeometryPath) {
        let derived = DerivedSceneData::prepare(path, self.wide());
        self.derived = derived;
    }

    pub(crate) fn wide(&self) -> Option<&SceneBuffers> {
        match &self.source {
            SceneSource::Wide(scene) => Some(scene),
            SceneSource::Empty | SceneSource::Resident(_) => None,
        }
    }

    pub(crate) fn resident_upload(&self) -> Option<&ResidentSceneCpu> {
        match &self.source {
            SceneSource::Resident(scene) => Some(scene),
            SceneSource::Empty | SceneSource::Wide(_) => None,
        }
    }

    pub(crate) fn resident_upload_mut(&mut self) -> Option<&mut ResidentSceneCpu> {
        match &mut self.source {
            SceneSource::Resident(scene) => Some(scene),
            SceneSource::Empty | SceneSource::Wide(_) => None,
        }
    }

    pub(crate) fn has_source(&self) -> bool {
        !matches!(self.source, SceneSource::Empty)
    }

    pub(crate) fn source_len(&self) -> Option<usize> {
        match &self.source {
            SceneSource::Empty => None,
            SceneSource::Wide(scene) => Some(scene.len()),
            SceneSource::Resident(scene) => Some(scene.len()),
        }
    }

    pub(crate) fn source_sh_degree(&self) -> Option<u8> {
        match &self.source {
            SceneSource::Empty => None,
            SceneSource::Wide(scene) => Some(scene.sh_degree),
            SceneSource::Resident(scene) => Some(scene.sh_degree),
        }
    }

    pub(crate) fn source_positions(&self) -> Option<&[Vec3f]> {
        match &self.source {
            SceneSource::Empty => None,
            SceneSource::Wide(scene) => Some(scene.positions.as_slice()),
            SceneSource::Resident(scene) => Some(scene.positions.as_ref()),
        }
    }

    pub(crate) fn direct_inputs(&self) -> Option<DirectSceneCpuView<'_>> {
        let scene = self.wide()?;
        match &self.derived {
            DerivedSceneData::Direct {
                world_covariances,
                world_covariance_terms,
                alpha_values,
            } => Some(DirectSceneCpuView {
                scene,
                world_covariances,
                world_covariance_terms,
                alpha_values,
            }),
            DerivedSceneData::None | DerivedSceneData::Paged { .. } => None,
        }
    }

    pub(crate) fn spatial_pages(&self) -> Option<&SpatialPageSet> {
        match &self.derived {
            DerivedSceneData::Paged { spatial_pages } => Some(spatial_pages),
            DerivedSceneData::None | DerivedSceneData::Direct { .. } => None,
        }
    }

    pub(crate) fn paged_order_inputs_mut(
        &mut self,
    ) -> Option<(&SceneBuffers, &SpatialPageSet, &mut Vec<u32>)> {
        let SceneSource::Wide(scene) = &self.source else {
            return None;
        };
        let DerivedSceneData::Paged { spatial_pages } = &self.derived else {
            return None;
        };
        Some((scene, spatial_pages, &mut self.preprocess_indices))
    }

    pub(crate) fn source_positions_and_preprocess_mut(
        &mut self,
    ) -> Option<(&[Vec3f], &mut Vec<u32>)> {
        let positions = match &self.source {
            SceneSource::Empty => return None,
            SceneSource::Wide(scene) => scene.positions.as_slice(),
            SceneSource::Resident(scene) => scene.positions.as_ref(),
        };
        Some((positions, &mut self.preprocess_indices))
    }

    pub(crate) fn preprocess_indices(&self) -> &[u32] {
        &self.preprocess_indices
    }

    pub(crate) fn preprocess_indices_mut(&mut self) -> &mut Vec<u32> {
        &mut self.preprocess_indices
    }

    pub(crate) fn swap_preprocess_indices(&mut self, indices: &mut Vec<u32>) {
        std::mem::swap(&mut self.preprocess_indices, indices);
    }

    #[cfg(test)]
    pub(crate) fn preprocess_capacity(&self) -> usize {
        self.preprocess_indices.capacity()
    }
}

pub(crate) fn default_spatial_pages(scene: &SceneBuffers) -> SpatialPageSet {
    let page_capacity = (scene.len() / 4).clamp(1, DEFAULT_PAGE_CAPACITY);
    let grid_axis = ((scene.len() as f32).cbrt().ceil() as usize).clamp(1, 8);
    spatial_pages::partition_scene_pages_with_coarse_cover(
        scene,
        page_capacity,
        grid_axis,
        DEFAULT_PAGED_ATLAS_SLOTS,
    )
}

#[cfg(test)]
mod tests {
    use gsplat_core::{SceneBuffers, Vec3f};

    use super::RendererSceneState;
    use crate::GeometryPath;
    use crate::scene::ResidentSceneCpu;

    fn scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0), Vec3f::new(0.1, 0.0, 1.2)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[-3.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn wide_and_resident_sources_are_structurally_exclusive() {
        let source = scene();
        let resident = ResidentSceneCpu::encode(&source).expect("resident scene");
        let mut state = RendererSceneState::empty();

        state.replace_wide(source, GeometryPath::SortedIndexDirect);
        assert!(state.wide().is_some());
        assert!(state.resident_upload().is_none());
        assert!(state.direct_inputs().is_some());
        assert!(state.spatial_pages().is_none());

        state.replace_resident(resident);
        assert!(state.wide().is_none());
        assert!(state.resident_upload().is_some());
        assert!(state.direct_inputs().is_none());
        assert!(state.spatial_pages().is_none());
    }

    #[test]
    fn path_rebuild_replaces_direct_and_paged_derived_data_as_one_variant() {
        let mut state = RendererSceneState::empty();
        state.replace_wide(scene(), GeometryPath::SortedIndexDirect);
        assert!(state.direct_inputs().is_some());

        state.rebuild_for_path(GeometryPath::PagedActiveAtlas);
        assert!(state.direct_inputs().is_none());
        assert!(state.spatial_pages().is_some());

        state.rebuild_for_path(GeometryPath::SortedIndexDirect);
        assert!(state.direct_inputs().is_some());
        assert!(state.spatial_pages().is_none());
    }

    #[test]
    fn exact_publish_clears_order_contents_without_discarding_reusable_capacity() {
        let mut state = RendererSceneState::empty();
        state.replace_wide(scene(), GeometryPath::SortedIndexDirect);
        state.preprocess_indices_mut().extend([1, 0]);
        let capacity = state.preprocess_capacity();

        state.clear_for_exact_runtime();

        assert!(!state.has_source());
        assert!(state.preprocess_indices().is_empty());
        assert_eq!(state.preprocess_capacity(), capacity);
    }
}
