//! WGPU Surface presentation and geometry-resource ownership.

use gsplat_core::Camera;

use crate::draw_pass::{SplatDraw, encode_splat_draw_into};
use crate::{
    Renderer, ResidentSceneError, ResidentScenePath, ResidentScenePreflight,
    ResidentSceneResources, SurfacePresenterError, create_resident_bind_group_layout,
    create_resident_pipeline, create_surface_instance, fit_surface_size, select_present_mode,
    surface_error_to_presenter, wgpu_label,
};

struct SurfaceAdapterContext {
    info: wgpu::AdapterInfo,
    limits: wgpu::Limits,
}

pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    resident_bind_group_layout: wgpu::BindGroupLayout,
    resident_pipeline: wgpu::RenderPipeline,
    surface_config: wgpu::SurfaceConfiguration,
    max_texture_dimension_2d: u32,
    instance_count: u32,
    resident_scene: ResidentSceneResources,
}
fn create_resident_scene_resources(
    device: &wgpu::Device,
    resident_bind_group_layout: &wgpu::BindGroupLayout,
    renderer: &Renderer,
) -> Result<ResidentSceneResources, SurfacePresenterError> {
    let scene = renderer
        .scene()
        .ok_or(SurfacePresenterError::SceneNotLoaded)?;
    let world_covariance_terms = renderer
        .world_covariance_terms
        .as_deref()
        .ok_or(SurfacePresenterError::SceneNotLoaded)?;
    let alpha_values = renderer
        .alpha_values
        .as_deref()
        .ok_or(SurfacePresenterError::SceneNotLoaded)?;
    ResidentSceneResources::new(
        device,
        resident_bind_group_layout,
        scene,
        world_covariance_terms,
        alpha_values,
        renderer.storage_profile(),
    )
    .map_err(SurfacePresenterError::from)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceResourcePlan {
    pub(crate) resident_preflight: ResidentScenePreflight,
    pub(crate) required_texture_dimension: u32,
}

impl SurfaceResourcePlan {
    pub(crate) fn validate(self) -> Result<(), ResidentSceneError> {
        if self.resident_preflight.path == ResidentScenePath::CapacityExceeded {
            return Err(ResidentSceneError::ResourceLimitExceeded(Box::new(
                self.resident_preflight,
            )));
        }
        Ok(())
    }
}

pub(crate) fn surface_resource_plan(
    scene_splats: usize,
    sh_degree: u8,
    width: u32,
    height: u32,
    limits: &wgpu::Limits,
    profile: crate::ResidentStorageProfile,
) -> Result<SurfaceResourcePlan, ResidentSceneError> {
    Ok(SurfaceResourcePlan {
        resident_preflight: crate::resident_scene_preflight_for_profile(
            scene_splats,
            sh_degree,
            limits,
            profile,
        )?,
        required_texture_dimension: width.max(height),
    })
}

fn surface_required_device_limits(
    adapter_limits: &wgpu::Limits,
    resource_plan: &SurfaceResourcePlan,
) -> Result<wgpu::Limits, SurfacePresenterError> {
    resource_plan.validate()?;
    if resource_plan.required_texture_dimension > adapter_limits.max_texture_dimension_2d {
        return Err(SurfacePresenterError::DeviceCreation(format!(
            "required texture dimension {} exceeds adapter limit {}",
            resource_plan.required_texture_dimension, adapter_limits.max_texture_dimension_2d
        )));
    }

    let required_storage_bytes = resource_plan
        .resident_preflight
        .requirements
        .iter()
        .map(|requirement| requirement.required_bytes)
        .max()
        .unwrap_or(0);
    let required_storage_binding_size = u32::try_from(required_storage_bytes).map_err(|_| {
        SurfacePresenterError::DeviceCreation(format!(
            "resident scene requires a {required_storage_bytes}-byte storage binding, exceeding the wgpu limit representation"
        ))
    })?;

    let mut required_limits = wgpu::Limits::downlevel_defaults();
    // Preserve the adapter's full resize headroom. SurfacePresenter may be
    // created for a small window and later resized to a larger display; only
    // storage/buffer limits are intentionally requested scene-by-scene.
    required_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    required_limits.max_storage_buffer_binding_size = required_limits
        .max_storage_buffer_binding_size
        .max(required_storage_binding_size);
    required_limits.max_buffer_size = required_limits.max_buffer_size.max(required_storage_bytes);
    crate::quantized::apply_storage_buffer_stage_headroom(&mut required_limits, adapter_limits);

    if !required_limits.check_limits(adapter_limits) {
        return Err(SurfacePresenterError::DeviceCreation(format!(
            "surface requirements exceed adapter capabilities; requested={required_limits:?}; adapter={adapter_limits:?}"
        )));
    }
    Ok(required_limits)
}

impl SurfacePresenter {
    /// Creates a presenter for an owned native window target.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_window<T>(
        target: T,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        Self::from_window_selected(target, width, height, renderer).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn from_window_selected<T>(
        target: T,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let instance = create_surface_instance();
        let surface = instance
            .create_surface(target)
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    /// Creates a presenter from raw handles supplied by an embedding platform.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the raw display and window handles remain valid until
    /// after the returned presenter is dropped.
    pub unsafe fn from_raw_handles(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        pollster::block_on(Self::from_raw_handles_selected(
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            renderer,
        ))
    }

    async fn from_raw_handles_selected(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        let instance = create_surface_instance();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|_| SurfacePresenterError::SurfaceCreation)?;

        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        Self::from_canvas_selected(canvas, width, height, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    async fn from_canvas_selected(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        let instance = create_surface_instance();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, renderer).await
    }

    async fn from_surface_async(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| SurfacePresenterError::NoAdapter)?;

        let adapter_limits = adapter.limits();
        let adapter_context = SurfaceAdapterContext {
            info: adapter.get_info(),
            limits: adapter_limits,
        };
        Self::from_surface_with_adapter_async(
            adapter,
            surface,
            width,
            height,
            renderer,
            adapter_context,
        )
        .await
    }

    async fn from_surface_with_adapter_async(
        adapter: wgpu::Adapter,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        renderer: &Renderer,
        adapter_context: SurfaceAdapterContext,
    ) -> Result<Self, SurfacePresenterError> {
        let SurfaceAdapterContext {
            info: adapter_info,
            limits: adapter_limits,
        } = adapter_context;
        // Plan against the adapter's physical limits first. Device creation
        // then requests only the resident scene's exact increase above
        // portable defaults rather than copying the adapter maximum wholesale.
        let scene = renderer
            .scene()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let resource_plan = surface_resource_plan(
            scene.len(),
            scene.sh_degree,
            width,
            height,
            &adapter_limits,
            renderer.storage_profile(),
        )?;
        let required_limits = surface_required_device_limits(&adapter_limits, &resource_plan)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-surface-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| {
                SurfacePresenterError::DeviceCreation(format!(
                    "{err}; adapter={adapter_info:?}; limits={adapter_limits:?}"
                ))
            })?;

        let caps = surface.get_capabilities(&adapter);
        let Some(format) = caps.formats.first().copied() else {
            return Err(SurfacePresenterError::NoSurfaceFormat);
        };
        let present_mode = select_present_mode(&caps);
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d.max(1);
        let (surface_width, surface_height) =
            fit_surface_size(width, height, max_texture_dimension_2d);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: surface_width,
            height: surface_height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &surface_config);
        if let Some(err) = error_scope.pop().await {
            return Err(SurfacePresenterError::SurfaceConfigure(err.to_string()));
        }

        let resident_bind_group_layout = create_resident_bind_group_layout(&device);
        let resident_pipeline =
            create_resident_pipeline(&device, &resident_bind_group_layout, format);
        let resident_scene =
            create_resident_scene_resources(&device, &resident_bind_group_layout, renderer)?;

        Ok(Self {
            surface,
            device,
            queue,
            resident_bind_group_layout,
            resident_pipeline,
            surface_config,
            max_texture_dimension_2d,
            instance_count: 0,
            resident_scene,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let (width, height) = fit_surface_size(width, height, self.max_texture_dimension_2d);
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub const fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        let latency = latency.clamp(1, 4);
        if self.surface_config.desired_maximum_frame_latency == latency {
            return;
        }

        self.surface_config.desired_maximum_frame_latency = latency;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn render_sorted_indices(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.instance_count = self.resident_scene.prepare_cpu(
            &self.queue,
            sorted_indices,
            camera,
            self.surface_config.width,
            self.surface_config.height,
            refresh_indices,
        )?;
        self.present_resident_scene()
    }

    /// Recreates resident GPU buffers for the renderer's current storage
    /// profile. The draw pipeline is unchanged; GPU-order resources are dropped
    /// and must be prepared again if that backend is still selected.
    pub(crate) fn rebuild_resident_scene(
        &mut self,
        renderer: &Renderer,
    ) -> Result<(), SurfacePresenterError> {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.resident_scene = create_resident_scene_resources(
            &self.device,
            &self.resident_bind_group_layout,
            renderer,
        )?;
        self.instance_count = 0;
        Ok(())
    }

    /// Pre-creates the resident GPU ordering pipelines and buffers outside a
    /// measured/presented frame. No sorting or drawing happens here.
    pub(crate) fn prepare_resident_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        self.resident_scene.ensure_gpu_order(&self.device)?;
        Ok(())
    }

    /// Generates and stably sorts depth pairs on this presenter's GPU,
    /// then draws from the resident pair buffer in the same submission.
    pub(crate) fn render_resident_gpu_order(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.instance_count = self.resident_scene.prepare_gpu(
            &self.device,
            &self.queue,
            camera,
            self.surface_config.width,
            self.surface_config.height,
        )?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-resident-gpu-order-encoder"),
            });
        {
            let gpu_order = self.resident_scene.gpu_order().ok_or_else(|| {
                ResidentSceneError::GpuOrderInitialization(
                    "GPU order resources were not initialized".to_owned(),
                )
            })?;
            if refresh_order {
                gpu_order.sorter.encode(&mut encoder);
            }
        }
        self.resident_scene
            .encode_project(&mut encoder, self.instance_count, true);

        let Some(frame) = self.acquire_surface_texture()? else {
            // A swapchain timeout must not discard a requested order refresh:
            // submit the compute work so the next acquired frame never reads
            // an uninitialized pair buffer.
            if refresh_order {
                self.queue.submit(Some(encoder.finish()));
            }
            return Ok(());
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "gsplat-surface-resident-gpu-order-draw-pass",
                view: &view,
                pipeline: &self.resident_pipeline,
                bind_group: &self.resident_scene.draw_bind_group,
                clear: wgpu::Color::BLACK,
                vertex_count: 6,
                instance_count: self.instance_count,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn present_resident_scene(&mut self) -> Result<(), SurfacePresenterError> {
        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(());
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-resident-encoder"),
            });
        self.resident_scene
            .encode_project(&mut encoder, self.instance_count, false);
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "gsplat-surface-resident-pass",
                view: &view,
                pipeline: &self.resident_pipeline,
                bind_group: &self.resident_scene.draw_bind_group,
                clear: wgpu::Color::BLACK,
                vertex_count: 6,
                instance_count: self.instance_count,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn acquire_surface_texture(
        &mut self,
    ) -> Result<Option<wgpu::SurfaceTexture>, SurfacePresenterError> {
        match self.surface.get_current_texture() {
            Ok(frame) => Ok(Some(frame)),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    Ok(frame) => Ok(Some(frame)),
                    Err(wgpu::SurfaceError::Timeout) => Ok(None),
                    Err(err) => Err(surface_error_to_presenter(err)),
                }
            }
            Err(wgpu::SurfaceError::Timeout) => Ok(None),
            Err(err) => Err(surface_error_to_presenter(err)),
        }
    }

    pub const fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter_limits(storage_bytes: u32, buffer_bytes: u64) -> wgpu::Limits {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_storage_buffer_binding_size = storage_bytes;
        adapter_limits.max_buffer_size = buffer_bytes;
        adapter_limits
    }

    fn resource_plan(
        scene_splats: usize,
        sh_degree: u8,
        limits: &wgpu::Limits,
    ) -> SurfaceResourcePlan {
        surface_resource_plan(
            scene_splats,
            sh_degree,
            716,
            1_600,
            limits,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("resource plan")
    }

    #[test]
    fn small_resident_scene_keeps_portable_limits_on_larger_adapter() {
        let mut adapter = adapter_limits(256 << 20, 512 << 20);
        adapter.max_texture_dimension_2d = 16_384;
        let plan = resource_plan(279_199, 3, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_texture_dimension_2d, 16_384);
        assert_eq!(
            requested.max_storage_buffer_binding_size,
            wgpu::Limits::downlevel_defaults().max_storage_buffer_binding_size
        );
        assert_eq!(
            requested.max_buffer_size,
            wgpu::Limits::downlevel_defaults().max_buffer_size
        );
    }

    #[test]
    fn larger_adapter_requests_exact_750k_degree_three_binding() {
        let adapter = adapter_limits(256 << 20, 512 << 20);
        let plan = resource_plan(750_000, 3, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffer_binding_size, 135_000_000);
        assert_eq!(requested.max_buffer_size, 256 << 20);
        assert_eq!(
            crate::resident_scene_preflight(750_000, 3, &requested)
                .expect("preflight")
                .path,
            crate::ResidentScenePath::Resident
        );
    }

    #[test]
    fn surface_larger_than_adapter_texture_limit_is_rejected() {
        let adapter = adapter_limits(256 << 20, 512 << 20);
        let plan = surface_resource_plan(
            279_199,
            3,
            adapter.max_texture_dimension_2d + 1,
            1_600,
            &adapter,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("resource plan");

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();

        assert!(matches!(error, SurfacePresenterError::DeviceCreation(_)));
        assert!(error.to_string().contains("required texture dimension"));
    }

    #[test]
    fn physical_128_mib_adapter_rejects_750k_degree_three_scene() {
        let adapter = wgpu::Limits::downlevel_defaults();
        let plan = resource_plan(750_000, 3, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::ResidentScene(ResidentSceneError::ResourceLimitExceeded(report)) =
            error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(report.effective_storage_binding_limit, 128 << 20);
        assert_eq!(report.max_resident_splats, 745_654);
        assert_eq!(report.requirements[2].required_bytes, 135_000_000);
    }

    #[test]
    fn resident_scene_above_256_mib_raises_binding_and_buffer_exactly() {
        let adapter = adapter_limits(512 << 20, 512 << 20);
        let plan = resource_plan(1_500_000, 3, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffer_binding_size, 270_000_000);
        assert_eq!(requested.max_buffer_size, 270_000_000);
    }

    #[test]
    fn surface_requests_webgpu_storage_buffer_count_when_adapter_allows() {
        let mut adapter = adapter_limits(256 << 20, 512 << 20);
        adapter.max_storage_buffers_per_shader_stage = 8;
        let plan = resource_plan(279_199, 3, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffers_per_shader_stage, 8);
    }
}
