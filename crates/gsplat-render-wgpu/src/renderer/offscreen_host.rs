//! Native offscreen target, readback, and compatibility draw orchestration.
//!
//! This host owns only GPU target mechanics and path-local draw resources.
//! `Renderer` remains the owner of scene/path transactions, Exact prepared
//! runtime state, semantic generations, ordering policy, and final statistics.

use std::sync::Arc;

use gsplat_core::{Camera, RendererConfig, SceneBuffers};

use crate::data::CameraCovarianceTerms;
use crate::direct_scene_gpu::{
    DirectSceneResources, create_direct_bind_group_layout, create_direct_pipeline,
};
use crate::scene::ResidentSceneCpu;
use crate::{
    RENDER_TARGET_FORMAT, RendererError, SpatialPageSet, offscreen, packed_gpu, paged_active_set,
    raster, refresh_paged_hot_colors, resident_gpu, wgpu_label,
};

struct OffscreenResidentPipelines {
    draw_pipeline: wgpu::RenderPipeline,
    draw_bind_group_layout: wgpu::BindGroupLayout,
    color_pipeline: wgpu::ComputePipeline,
    color_bind_group_layout: wgpu::BindGroupLayout,
}

pub(crate) struct OffscreenHost {
    adapter_info: wgpu::AdapterInfo,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    target: offscreen::OffscreenTarget,
    max_texture_dimension_2d: u32,
    direct_pipeline: wgpu::RenderPipeline,
    direct_bind_group_layout: wgpu::BindGroupLayout,
    direct_scene: Option<DirectSceneResources>,
    packed_pipeline: wgpu::RenderPipeline,
    packed_bind_group_layout: wgpu::BindGroupLayout,
    resident_pipelines: Option<OffscreenResidentPipelines>,
    resident_scene: Option<resident_gpu::ResidentGpuResources>,
    resident_draw_bind_group: Option<wgpu::BindGroup>,
    paged_active_set: Option<paged_active_set::PagedActiveSet>,
}

impl OffscreenHost {
    pub(crate) fn create(config: &RendererConfig) -> Result<Self, RendererError> {
        pollster::block_on(Self::create_async(config))
    }

    async fn create_async(config: &RendererConfig) -> Result<Self, RendererError> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| RendererError::GpuRasterizerUnavailable)?;

        let adapter_info = adapter.get_info();
        let required_limits = offscreen_device_limits(config, &adapter.limits())?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-render-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;
        let target = offscreen::OffscreenTarget::new(
            &device,
            config.width,
            config.height,
            max_texture_dimension_2d,
        )?;
        let direct_bind_group_layout = create_direct_bind_group_layout(&device);
        let direct_pipeline =
            create_direct_pipeline(&device, &direct_bind_group_layout, RENDER_TARGET_FORMAT);
        let packed_bind_group_layout = packed_gpu::create_packed_bind_group_layout(&device);
        let packed_pipeline = packed_gpu::create_packed_pipeline(
            &device,
            &packed_bind_group_layout,
            RENDER_TARGET_FORMAT,
        );
        let resident_pipelines = (device.limits().max_storage_buffers_per_shader_stage
            >= resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS)
            .then(|| {
                let draw_bind_group_layout =
                    resident_gpu::create_resident_draw_bind_group_layout(&device);
                let draw_pipeline = resident_gpu::create_resident_draw_pipeline(
                    &device,
                    &draw_bind_group_layout,
                    RENDER_TARGET_FORMAT,
                );
                let color_bind_group_layout =
                    resident_gpu::create_resident_color_bind_group_layout(&device);
                let color_pipeline =
                    resident_gpu::create_resident_color_pipeline(&device, &color_bind_group_layout);
                OffscreenResidentPipelines {
                    draw_pipeline,
                    draw_bind_group_layout,
                    color_pipeline,
                    color_bind_group_layout,
                }
            });

        Ok(Self {
            adapter_info,
            device,
            queue,
            target,
            max_texture_dimension_2d,
            direct_pipeline,
            direct_bind_group_layout,
            direct_scene: None,
            packed_pipeline,
            packed_bind_group_layout,
            resident_pipelines,
            resident_scene: None,
            resident_draw_bind_group: None,
            paged_active_set: None,
        })
    }

    pub(crate) fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    pub(crate) fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    pub(crate) fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    pub(crate) fn target_view(&self) -> &wgpu::TextureView {
        self.target.view()
    }

    pub(crate) fn target_size(&self) -> (u32, u32) {
        self.target.size()
    }

    pub(crate) fn max_texture_dimension_2d(&self) -> u32 {
        self.max_texture_dimension_2d
    }

    pub(crate) fn clear_scene_resources(&mut self) {
        self.direct_scene = None;
        self.resident_scene = None;
        self.resident_draw_bind_group = None;
        self.paged_active_set = None;
    }

    pub(crate) fn ensure_output_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), RendererError> {
        self.target
            .ensure_size(&self.device, width, height, self.max_texture_dimension_2d)
    }

    pub(crate) fn render_direct_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
    ) -> Result<(), RendererError> {
        self.ensure_output_target(config.width, config.height)?;
        if self.direct_scene.is_none() {
            let candidate = DirectSceneResources::new(
                &self.device,
                &self.direct_bind_group_layout,
                scene,
                world_covariance_terms,
                alpha_values,
            )?;
            self.direct_scene = Some(candidate);
        }
        let direct_scene = self
            .direct_scene
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let instance_count = direct_scene
            .prepare_cpu(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let commands = raster::encode_splat_draw(
            &self.device,
            "gsplat-offscreen-direct-encoder",
            raster::SplatDraw {
                pass_label: "gsplat-offscreen-direct-pass",
                view: self.target_view(),
                pipeline: &self.direct_pipeline,
                bind_group: direct_scene.cpu_bind_group(),
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: raster::QUAD_VERTEX_COUNT,
                instance_count,
            },
        );
        self.queue.submit(Some(commands));
        Ok(())
    }

    pub(crate) fn render_packed_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &ResidentSceneCpu,
    ) -> Result<(), RendererError> {
        self.ensure_output_target(config.width, config.height)?;
        let resident_pipelines = self.resident_pipelines.as_ref().ok_or_else(|| {
            resident_gpu::ResidentGpuError::StorageBindingCountUnsupported(
                self.device.limits().max_storage_buffers_per_shader_stage,
            )
        })?;
        // Keep upload staging so Packed -> other path -> Packed can recreate
        // the same complete GPU scene. Publish both cached resources only
        // after the full candidate has been constructed.
        if self.resident_scene.is_none() {
            let resident = resident_gpu::ResidentGpuResources::new(
                &self.device,
                &resident_pipelines.color_bind_group_layout,
                scene,
            )?;
            let draw_bind_group = resident.create_offscreen_draw_bind_group(
                &self.device,
                &resident_pipelines.draw_bind_group_layout,
            );
            self.resident_scene = Some(resident);
            self.resident_draw_bind_group = Some(draw_bind_group);
        }
        let instance_count = self
            .resident_scene
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?
            .prepare_cpu_order(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-offscreen-resident-encoder"),
            });
        self.resident_scene
            .as_mut()
            .ok_or(RendererError::GpuDeviceCreation)?
            .encode_color_resolve_if_needed(
                &self.queue,
                &resident_pipelines.color_pipeline,
                &mut encoder,
                camera,
                self.device.limits().max_compute_workgroups_per_dimension,
            )?;
        raster::encode_splat_draw_into(
            &mut encoder,
            &raster::SplatDraw {
                pass_label: "gsplat-offscreen-resident-pass",
                view: self.target_view(),
                pipeline: &resident_pipelines.draw_pipeline,
                bind_group: self
                    .resident_draw_bind_group
                    .as_ref()
                    .ok_or(RendererError::GpuDeviceCreation)?,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: raster::QUAD_VERTEX_COUNT,
                instance_count,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    pub(crate) fn ensure_paged_active_set(
        &mut self,
        scene: &SceneBuffers,
        pages: &SpatialPageSet,
        camera: &Camera,
    ) -> Result<(), RendererError> {
        if self.paged_active_set.is_none() {
            let candidate = paged_active_set::PagedActiveSet::new(
                &self.device,
                &self.packed_bind_group_layout,
                scene,
                pages.clone(),
            )?;
            self.paged_active_set = Some(candidate);
        }

        self.paged_active_set
            .as_mut()
            .ok_or(RendererError::InvalidScene)?
            .sync(&self.queue, scene, camera)
    }

    pub(crate) fn paged_active_entries(&self) -> Result<Vec<(u32, u32)>, RendererError> {
        self.paged_active_set
            .as_ref()
            .map(|active_set| active_set.atlas.active_entries())
            .ok_or(RendererError::InvalidScene)
    }

    pub(crate) fn render_paged_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
    ) -> Result<(), RendererError> {
        self.ensure_output_target(config.width, config.height)?;
        let paged = self
            .paged_active_set
            .as_mut()
            .ok_or(RendererError::GpuDeviceCreation)?;
        refresh_paged_hot_colors(&self.queue, &mut paged.atlas, scene, camera);
        let instance_count = paged
            .atlas
            .resources
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let paged = self
            .paged_active_set
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let commands = raster::encode_splat_draw(
            &self.device,
            "gsplat-offscreen-paged-encoder",
            raster::SplatDraw {
                pass_label: "gsplat-offscreen-paged-pass",
                view: self.target_view(),
                pipeline: &self.packed_pipeline,
                bind_group: &paged.atlas.resources.bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: raster::QUAD_VERTEX_COUNT,
                instance_count,
            },
        );
        self.queue.submit(Some(commands));
        Ok(())
    }

    pub(crate) fn readback_rgba8(&self) -> Result<Vec<u8>, RendererError> {
        offscreen::readback_rgba8(&self.device, &self.queue, &self.target)
    }

    #[cfg(test)]
    pub(crate) fn ensure_output_target_with_device_for_test(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(), RendererError> {
        self.target
            .ensure_size(device, width, height, self.max_texture_dimension_2d)
    }

    #[cfg(test)]
    pub(crate) fn paged_active_set_for_test(&self) -> Option<&paged_active_set::PagedActiveSet> {
        self.paged_active_set.as_ref()
    }
}

pub(crate) fn offscreen_device_limits(
    config: &RendererConfig,
    adapter_limits: &wgpu::Limits,
) -> Result<wgpu::Limits, RendererError> {
    if config.width == 0 || config.height == 0 {
        return Err(RendererError::InvalidConfig);
    }

    let requested_dimension = config.width.max(config.height);
    if requested_dimension > adapter_limits.max_texture_dimension_2d {
        return Err(RendererError::GpuDimensionsUnsupported {
            width: config.width,
            height: config.height,
            max_dimension: adapter_limits.max_texture_dimension_2d,
        });
    }

    let mut required_limits = wgpu::Limits::downlevel_defaults();
    // Preserve the adapter's storage and texture headroom because the host can
    // switch paths and load a scene only after device creation.
    required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    required_limits.max_storage_buffer_binding_size =
        adapter_limits.max_storage_buffer_binding_size;
    required_limits.max_buffer_size = adapter_limits.max_buffer_size;
    required_limits.max_storage_buffers_per_shader_stage =
        adapter_limits.max_storage_buffers_per_shader_stage;
    if !required_limits.check_limits(adapter_limits) {
        return Err(RendererError::GpuDeviceCreation);
    }
    Ok(required_limits)
}

#[cfg(test)]
mod tests {
    #[test]
    fn owner_contains_target_readback_and_all_compatibility_draws() {
        let source = include_str!("offscreen_host.rs");
        for required in [
            "OffscreenTarget",
            "readback_rgba8",
            "gsplat-offscreen-direct-pass",
            "gsplat-offscreen-resident-pass",
            "gsplat-offscreen-paged-pass",
            "create_command_encoder",
            "queue.submit",
        ] {
            assert!(
                source.contains(required),
                "offscreen host is missing {required}"
            );
        }

        let facade = include_str!("../lib.rs");
        for moved_owner in [
            "struct GpuRasterizer",
            "struct OffscreenResidentPipelines",
            "gsplat-offscreen-direct-pass",
            "gsplat-offscreen-resident-pass",
            "gsplat-offscreen-paged-pass",
        ] {
            assert!(
                !facade.contains(moved_owner),
                "crate facade still owns {moved_owner}"
            );
        }
    }
}
