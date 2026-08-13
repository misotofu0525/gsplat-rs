//! Offscreen GPU rasterizer used by Renderer::render_frame.

use gsplat_core::{Camera, RendererConfig, SceneBuffers};

use crate::draw_pass;
use crate::error::RendererError;
use crate::math::CameraCovarianceTerms;
use crate::resident::{
    ResidentSceneResources, create_resident_bind_group_layout, create_resident_pipeline,
};
use crate::timing::wgpu_label;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const RENDER_TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(crate) struct GpuRasterizer {
    pub(crate) adapter_info: wgpu::AdapterInfo,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    output_size: (u32, u32),
    max_texture_dimension_2d: u32,
    resident_pipeline: wgpu::RenderPipeline,
    resident_bind_group_layout: wgpu::BindGroupLayout,
    resident_scene: Option<ResidentSceneResources>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GpuRasterizer {
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

        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;

        let (output_texture, output_view) = create_output_target(
            &device,
            config.width,
            config.height,
            max_texture_dimension_2d,
        )?;
        let resident_bind_group_layout = create_resident_bind_group_layout(&device);
        let resident_pipeline =
            create_resident_pipeline(&device, &resident_bind_group_layout, RENDER_TARGET_FORMAT);

        Ok(Self {
            adapter_info,
            device,
            queue,
            output_texture,
            output_view,
            output_size: (config.width, config.height),
            max_texture_dimension_2d,
            resident_pipeline,
            resident_bind_group_layout,
            resident_scene: None,
        })
    }

    pub(crate) fn clear_scene_resources(&mut self) {
        self.resident_scene = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_resident_sorted_indices(
        &mut self,
        config: RendererConfig,
        sorted_indices: &[u32],
        camera: &Camera,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
        profile: crate::ResidentStorageProfile,
    ) -> Result<(), RendererError> {
        self.ensure_output_target(config.width, config.height)?;
        if self.resident_scene.is_none() {
            self.resident_scene = Some(ResidentSceneResources::new(
                &self.device,
                &self.resident_bind_group_layout,
                scene,
                world_covariance_terms,
                alpha_values,
                profile,
            )?);
        }
        let resident_scene = self
            .resident_scene
            .as_ref()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let instance_count = resident_scene
            .prepare_cpu(
                &self.queue,
                sorted_indices,
                camera,
                config.width,
                config.height,
                true,
            )
            .map_err(|_| RendererError::GpuDeviceCreation)?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-offscreen-resident-encoder"),
            });
        resident_scene.encode_project(&mut encoder, instance_count, false);
        draw_pass::encode_splat_draw_into(
            &mut encoder,
            &draw_pass::SplatDraw {
                pass_label: "gsplat-offscreen-resident-pass",
                view: &self.output_view,
                pipeline: &self.resident_pipeline,
                bind_group: &resident_scene.draw_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: 6,
                instance_count,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn ensure_gpu_order(&mut self) -> Result<(), crate::ResidentSceneError> {
        let scene = self.resident_scene.as_mut().ok_or_else(|| {
            crate::ResidentSceneError::GpuOrderInitialization("no resident scene".to_owned())
        })?;
        scene.ensure_gpu_order(&self.device)
    }

    pub(crate) fn readback_rgba8(&mut self) -> Result<Vec<u8>, RendererError> {
        use std::sync::mpsc;

        let (width, height) = self.output_size;
        if width == 0 || height == 0 {
            return Err(RendererError::InvalidConfig);
        }

        let bytes_per_pixel = 4_u32;
        let unpadded_bytes_per_row = width.saturating_mul(bytes_per_pixel);
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align).saturating_mul(align);
        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("splat-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("splat-readback-encoder"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.output_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
        }

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

        match rx.recv() {
            Ok(Ok(())) => {}
            _ => return Err(RendererError::GpuReadback),
        }

        let mapped = slice.get_mapped_range();
        let unpadded = unpadded_bytes_per_row as usize;
        let padded = padded_bytes_per_row as usize;
        let mut out = vec![0_u8; unpadded.saturating_mul(height as usize)];

        for row in 0..(height as usize) {
            let src_start = row * padded;
            let dst_start = row * unpadded;
            out[dst_start..dst_start + unpadded]
                .copy_from_slice(&mapped[src_start..src_start + unpadded]);
        }

        drop(mapped);
        readback.unmap();
        Ok(out)
    }

    pub(crate) fn ensure_output_target(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), RendererError> {
        if self.output_size == (width, height) {
            return Ok(());
        }

        let (texture, view) =
            create_output_target(&self.device, width, height, self.max_texture_dimension_2d)?;
        self.output_texture = texture;
        self.output_view = view;
        self.output_size = (width, height);
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
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
    // Preserve resize headroom instead of freezing the device to the initial
    // render-target size.
    required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    crate::quantized::apply_storage_buffer_stage_headroom(&mut required_limits, adapter_limits);
    if !required_limits.check_limits(adapter_limits) {
        return Err(RendererError::GpuDeviceCreation);
    }
    Ok(required_limits)
}

#[cfg(not(target_arch = "wasm32"))]
fn create_output_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    max_texture_dimension_2d: u32,
) -> Result<(wgpu::Texture, wgpu::TextureView), RendererError> {
    if width > max_texture_dimension_2d || height > max_texture_dimension_2d {
        return Err(RendererError::GpuDimensionsUnsupported {
            width,
            height,
            max_dimension: max_texture_dimension_2d,
        });
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: wgpu_label("splat-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RENDER_TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ok((texture, view))
}
