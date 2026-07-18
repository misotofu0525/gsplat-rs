//! Page payload boundary for the local full-resident paging prototype.
//!
//! `LocalScenePageSource` still retains the complete source `SceneBuffers` in
//! its caller. It is not disk/network streaming. The boundary keeps source
//! extraction and packing out of the fixed-slot GPU atlas so a future bounded
//! source can provide the same decoded payload without changing GPU upload.

use gsplat_core::SceneBuffers;

use crate::packed_atlas::{
    LogScaleRange, PackedSceneCpu, SceneBounds, pack_scene_with_encoding, scene_sh_scales,
};
use crate::page_atlas::extract_page_scene;
use crate::residency::AttributeLod;
use crate::spatial_pages::{PageId, SpatialPage, SpatialPageSet};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageEncoding {
    pub(crate) scene_bounds: SceneBounds,
    pub(crate) log_scale_range: LogScaleRange,
    pub(crate) sh_scales: [f32; 3],
}

impl PageEncoding {
    pub(crate) fn from_scene(scene: &SceneBuffers) -> Self {
        Self {
            scene_bounds: SceneBounds::from_positions(&scene.positions),
            log_scale_range: LogScaleRange::from_scales(&scene.scale_xyz),
            sh_scales: scene_sh_scales(scene),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecodedPagePayload {
    pub(crate) page_id: PageId,
    pub(crate) source_indices: Vec<u32>,
    pub(crate) packed: PackedSceneCpu,
}

impl DecodedPagePayload {
    pub(crate) fn from_local_scene(
        scene: &SceneBuffers,
        page: &SpatialPage,
        encoding: PageEncoding,
        attribute_lod: AttributeLod,
    ) -> Self {
        let extracted = extract_page_scene(scene, page);
        let mut packed = pack_scene_with_encoding(
            &extracted,
            encoding.scene_bounds,
            encoding.log_scale_range,
            Some(encoding.sh_scales),
        );
        if attribute_lod == AttributeLod::Degree0 {
            packed.sh_degree = 0;
            packed.sh_sidecars.clear();
        }
        Self {
            page_id: page.id,
            source_indices: page.splat_indices.clone(),
            packed,
        }
    }
}

pub(crate) trait PageSource {
    fn pages(&self) -> &SpatialPageSet;

    fn decode_page(
        &self,
        page_id: PageId,
        attribute_lod: AttributeLod,
    ) -> Option<DecodedPagePayload>;
}

pub(crate) struct LocalScenePageSource<'a> {
    scene: &'a SceneBuffers,
    pages: &'a SpatialPageSet,
    encoding: PageEncoding,
}

impl<'a> LocalScenePageSource<'a> {
    pub(crate) fn new(
        scene: &'a SceneBuffers,
        pages: &'a SpatialPageSet,
        encoding: PageEncoding,
    ) -> Self {
        Self {
            scene,
            pages,
            encoding,
        }
    }
}

impl PageSource for LocalScenePageSource<'_> {
    fn pages(&self) -> &SpatialPageSet {
        self.pages
    }

    fn decode_page(
        &self,
        page_id: PageId,
        attribute_lod: AttributeLod,
    ) -> Option<DecodedPagePayload> {
        self.pages.page(page_id).map(|page| {
            DecodedPagePayload::from_local_scene(self.scene, page, self.encoding, attribute_lod)
        })
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{SceneBuffers, Vec3f};

    use super::*;
    use crate::spatial_pages::partition_scene_pages;

    fn sample_scene() -> SceneBuffers {
        SceneBuffers {
            positions: (0..6)
                .map(|index| Vec3f::new(index as f32 * 0.25, 0.0, 1.0))
                .collect(),
            opacity: vec![2.0; 6],
            scale_xyz: vec![[-3.0, -2.5, -2.0]; 6],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 6],
            color_dc: vec![[0.2, -0.05, 0.1]; 6],
            sh_degree: 1,
            sh_rest: Some(vec![0.05; 6 * 9]),
        }
    }

    #[test]
    fn local_source_payload_matches_shared_encoding_and_lod() {
        let scene = sample_scene();
        let pages = partition_scene_pages(&scene, 3, 2);
        let encoding = PageEncoding::from_scene(&scene);
        let source = LocalScenePageSource::new(&scene, &pages, encoding);
        let page = &pages.pages[0];

        let full = source
            .decode_page(page.id, AttributeLod::Degree3)
            .expect("page exists");
        let extracted = extract_page_scene(&scene, page);
        let expected = pack_scene_with_encoding(
            &extracted,
            encoding.scene_bounds,
            encoding.log_scale_range,
            Some(encoding.sh_scales),
        );
        assert_eq!(full.page_id, page.id);
        assert_eq!(full.source_indices, page.splat_indices);
        assert_eq!(full.packed, expected);
        assert_eq!(full.packed.sh_degree, 1);
        assert_eq!(full.packed.sh_sidecars.len(), page.splat_count());

        let degree_zero = source
            .decode_page(page.id, AttributeLod::Degree0)
            .expect("page exists");
        assert_eq!(degree_zero.source_indices, page.splat_indices);
        assert_eq!(degree_zero.packed.sh_degree, 0);
        assert!(degree_zero.packed.sh_sidecars.is_empty());
        assert_eq!(degree_zero.packed.hot, full.packed.hot);
    }
}
