//! Standalone diagnostic Paged execution for a native/WebGPU Surface.
//!
//! This module owns the Paged-only pipeline, active set, CPU preprocessing and
//! ordering scratch, draw encoding, and instance count. Surface acquisition,
//! submission, presentation, capture, and path-switch coordination remain in
//! `SurfacePresenter`.

use gsplat_core::{Camera, SceneBuffers};
use gsplat_sort::CpuSortBackend;

use crate::paged_active_set::PagedActiveSet;
use crate::raster::{QUAD_VERTEX_COUNT, SplatDraw, encode_splat_draw_into};
use crate::{
    SpatialPageSet, SurfacePresenterError, packed_gpu, preprocess_paged_visible_into,
    refresh_paged_hot_colors,
};

struct StandalonePagedScene {
    active_set: PagedActiveSet,
    sort_backend: CpuSortBackend,
    depth_keys: Vec<u32>,
    sorted_indices: Vec<u32>,
}

pub(crate) struct PreparedStandalonePagedScene(Box<StandalonePagedScene>);

impl PreparedStandalonePagedScene {
    pub(crate) fn addressable_splat_count(&self) -> usize {
        self.0.active_set.atlas.resources.capacity
    }
}

pub(crate) struct StandalonePagedRuntime {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    scene: Option<Box<StandalonePagedScene>>,
    instance_count: u32,
}

impl StandalonePagedRuntime {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = packed_gpu::create_packed_bind_group_layout(device);
        let pipeline = packed_gpu::create_packed_pipeline(device, &bind_group_layout, format);
        Self {
            bind_group_layout,
            pipeline,
            scene: None,
            instance_count: 0,
        }
    }

    pub(crate) fn prepare_scene_candidate(
        &self,
        device: &wgpu::Device,
        scene: &SceneBuffers,
        pages: SpatialPageSet,
    ) -> Result<PreparedStandalonePagedScene, SurfacePresenterError> {
        let active_set = PagedActiveSet::new(device, &self.bind_group_layout, scene, pages)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        Ok(PreparedStandalonePagedScene(Box::new(
            StandalonePagedScene {
                active_set,
                sort_backend: CpuSortBackend::default(),
                depth_keys: Vec::new(),
                sorted_indices: Vec::new(),
            },
        )))
    }

    pub(crate) fn publish_scene(&mut self, prepared: PreparedStandalonePagedScene) {
        self.scene = Some(prepared.0);
        self.instance_count = 0;
    }

    pub(crate) fn clear_scene(&mut self) {
        self.scene = None;
        self.instance_count = 0;
    }

    pub(crate) const fn instance_count(&self) -> u32 {
        self.instance_count
    }

    #[cfg(test)]
    pub(crate) fn slot_counts(&self) -> Option<(usize, usize)> {
        self.scene.as_ref().map(|scene| {
            (
                scene.active_set.atlas.slot_count(),
                scene.active_set.atlas.occupied_slot_count(),
            )
        })
    }

    pub(crate) fn prepare_frame(
        &mut self,
        queue: &wgpu::Queue,
        source_scene: &SceneBuffers,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        let scene = self
            .scene
            .as_mut()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        scene
            .active_set
            .sync(queue, source_scene, camera)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        let entries = scene.active_set.atlas.active_entries();
        preprocess_paged_visible_into(
            source_scene,
            &entries,
            camera,
            &mut scene.depth_keys,
            &mut scene.sorted_indices,
        )
        .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        scene
            .sort_backend
            .sort_values_by_keys(&scene.depth_keys, &mut scene.sorted_indices)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        refresh_paged_hot_colors(queue, &mut scene.active_set.atlas, source_scene, camera);
        self.instance_count = scene
            .active_set
            .atlas
            .resources
            .prepare(queue, &scene.sorted_indices, camera, width, height, true)
            .map_err(SurfacePresenterError::from)?;
        Ok(())
    }

    pub(crate) fn encode_draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> Result<(), SurfacePresenterError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        encode_splat_draw_into(
            encoder,
            &SplatDraw {
                pass_label: "gsplat-surface-paged-pass",
                view,
                pipeline: &self.pipeline,
                bind_group: &scene.active_set.atlas.resources.bind_group,
                clear: wgpu::Color::BLACK,
                vertex_count: QUAD_VERTEX_COUNT,
                instance_count: self.instance_count,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn standalone_paged_owner_contains_the_complete_private_execution_graph() {
        let source = include_str!("standalone_paged_runtime.rs");
        for required in [
            "PagedActiveSet",
            "CpuSortBackend",
            "preprocess_paged_visible_into",
            "refresh_paged_hot_colors",
            "create_packed_bind_group_layout",
            "create_packed_pipeline",
            "gsplat-surface-paged-pass",
        ] {
            assert!(
                source.contains(required),
                "Paged owner is missing {required}"
            );
        }
    }
}
