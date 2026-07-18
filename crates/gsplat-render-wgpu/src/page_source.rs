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
    pub(crate) encoding: PageEncoding,
    pub(crate) packed: PackedSceneCpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageSourceError {
    PageNotFound(PageId),
    SourceIndexOutOfBounds {
        page_id: PageId,
        source_index: u32,
        scene_splat_count: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PagePayloadError {
    EmptyPage,
    PageTooLarge {
        splat_count: usize,
        page_capacity: usize,
    },
    CountMismatch {
        source_indices: usize,
        packed_splats: usize,
        hot_records: usize,
    },
    SourceIndexOutOfBounds {
        source_index: u32,
        scene_splat_count: usize,
    },
    EncodingMismatch,
    SidecarCountMismatch {
        expected: usize,
        actual: usize,
    },
}

impl DecodedPagePayload {
    pub(crate) fn from_local_scene(
        scene: &SceneBuffers,
        page: &SpatialPage,
        encoding: PageEncoding,
        attribute_lod: AttributeLod,
    ) -> Result<Self, PageSourceError> {
        if let Some(&source_index) = page
            .splat_indices
            .iter()
            .find(|&&index| index as usize >= scene.len())
        {
            return Err(PageSourceError::SourceIndexOutOfBounds {
                page_id: page.id,
                source_index,
                scene_splat_count: scene.len(),
            });
        }
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
        Ok(Self {
            page_id: page.id,
            source_indices: page.splat_indices.clone(),
            encoding,
            packed,
        })
    }

    pub(crate) fn validate(
        &self,
        scene_splat_count: usize,
        page_capacity: usize,
        atlas_encoding: PageEncoding,
    ) -> Result<(), PagePayloadError> {
        let splat_count = self.source_indices.len();
        if splat_count == 0 {
            return Err(PagePayloadError::EmptyPage);
        }
        if splat_count > page_capacity {
            return Err(PagePayloadError::PageTooLarge {
                splat_count,
                page_capacity,
            });
        }
        if self.packed.splat_count != splat_count || self.packed.hot.len() != splat_count {
            return Err(PagePayloadError::CountMismatch {
                source_indices: splat_count,
                packed_splats: self.packed.splat_count,
                hot_records: self.packed.hot.len(),
            });
        }
        if let Some(&source_index) = self
            .source_indices
            .iter()
            .find(|&&index| index as usize >= scene_splat_count)
        {
            return Err(PagePayloadError::SourceIndexOutOfBounds {
                source_index,
                scene_splat_count,
            });
        }
        if self.encoding != atlas_encoding
            || self.packed.bounds != atlas_encoding.scene_bounds
            || self.packed.log_scale_range != atlas_encoding.log_scale_range
            || self.packed.sh_scales != atlas_encoding.sh_scales
        {
            return Err(PagePayloadError::EncodingMismatch);
        }
        let expected_sidecars = usize::from(self.packed.sh_degree > 0) * splat_count;
        if self.packed.sh_sidecars.len() != expected_sidecars {
            return Err(PagePayloadError::SidecarCountMismatch {
                expected: expected_sidecars,
                actual: self.packed.sh_sidecars.len(),
            });
        }
        Ok(())
    }
}

pub(crate) trait PageSource {
    fn pages(&self) -> &SpatialPageSet;

    fn decode_page(
        &self,
        page_id: PageId,
        attribute_lod: AttributeLod,
    ) -> Result<DecodedPagePayload, PageSourceError>;
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
    ) -> Result<DecodedPagePayload, PageSourceError> {
        let page = self
            .pages
            .page(page_id)
            .ok_or(PageSourceError::PageNotFound(page_id))?;
        DecodedPagePayload::from_local_scene(self.scene, page, self.encoding, attribute_lod)
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

    #[test]
    fn local_source_reports_missing_page_and_source_bounds() {
        let scene = sample_scene();
        let pages = partition_scene_pages(&scene, 3, 2);
        let encoding = PageEncoding::from_scene(&scene);
        let source = LocalScenePageSource::new(&scene, &pages, encoding);
        assert_eq!(
            source.decode_page(PageId(u32::MAX), AttributeLod::Degree0),
            Err(PageSourceError::PageNotFound(PageId(u32::MAX)))
        );

        let mut invalid_pages = pages.clone();
        let page = &mut invalid_pages.pages[0];
        page.splat_indices[0] = scene.len() as u32;
        let page_id = page.id;
        let invalid_source = LocalScenePageSource::new(&scene, &invalid_pages, encoding);
        assert_eq!(
            invalid_source.decode_page(page_id, AttributeLod::Degree0),
            Err(PageSourceError::SourceIndexOutOfBounds {
                page_id,
                source_index: scene.len() as u32,
                scene_splat_count: scene.len(),
            })
        );
    }

    #[test]
    fn decoded_payload_validates_count_bounds_encoding_and_capacity() {
        let scene = sample_scene();
        let pages = partition_scene_pages(&scene, 3, 2);
        let encoding = PageEncoding::from_scene(&scene);
        let source = LocalScenePageSource::new(&scene, &pages, encoding);
        let payload = source
            .decode_page(pages.pages[0].id, AttributeLod::Degree3)
            .expect("valid page");
        payload
            .validate(scene.len(), pages.page_capacity, encoding)
            .expect("valid payload");

        let mut count_mismatch = payload.clone();
        count_mismatch.source_indices.pop();
        assert!(matches!(
            count_mismatch.validate(scene.len(), pages.page_capacity, encoding),
            Err(PagePayloadError::CountMismatch { .. })
        ));

        let mut source_out_of_bounds = payload.clone();
        source_out_of_bounds.source_indices[0] = scene.len() as u32;
        assert_eq!(
            source_out_of_bounds.validate(scene.len(), pages.page_capacity, encoding),
            Err(PagePayloadError::SourceIndexOutOfBounds {
                source_index: scene.len() as u32,
                scene_splat_count: scene.len(),
            })
        );

        let mut encoding_mismatch = payload.clone();
        encoding_mismatch.encoding.scene_bounds.min[0] -= 1.0;
        assert_eq!(
            encoding_mismatch.validate(scene.len(), pages.page_capacity, encoding),
            Err(PagePayloadError::EncodingMismatch)
        );

        let mut packed_encoding_mismatch = payload.clone();
        packed_encoding_mismatch.packed.bounds.min[0] -= 1.0;
        assert_eq!(
            packed_encoding_mismatch.validate(scene.len(), pages.page_capacity, encoding),
            Err(PagePayloadError::EncodingMismatch)
        );

        assert_eq!(
            payload.validate(scene.len(), payload.source_indices.len() - 1, encoding),
            Err(PagePayloadError::PageTooLarge {
                splat_count: payload.source_indices.len(),
                page_capacity: payload.source_indices.len() - 1,
            })
        );

        let mut sidecar_mismatch = payload.clone();
        sidecar_mismatch.packed.sh_sidecars.pop();
        assert!(matches!(
            sidecar_mismatch.validate(scene.len(), pages.page_capacity, encoding),
            Err(PagePayloadError::SidecarCountMismatch { .. })
        ));
    }
}
