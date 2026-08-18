//! WGPU renderer with a SortedAlpha reference path.

mod draw_pass;
mod error;
mod math;
#[cfg(not(target_arch = "wasm32"))]
mod offscreen;
mod preprocess;
mod project;
mod quantized;
mod resident;
mod resident_gpu_order;
mod surface;
mod surface_adaptive;
mod surface_async;
mod surface_presenter;
mod surface_session;
mod timing;

#[cfg(test)]
mod cpu_geometry;

#[cfg(test)]
use cpu_geometry::{GpuInstance, build_instances_into};

pub use error::{RendererError, SurfacePresenterError};
pub use preprocess::PreprocessOutput;
pub use quantized::ResidentStorageProfile;
pub use resident::{
    ResidentSceneError, ResidentScenePath, ResidentScenePreflight, ResidentSceneRemediation,
    ResidentSceneResource, ResidentSceneResourceRequirement, resident_scene_preflight,
    resident_scene_preflight_for_profile,
};
pub use surface_presenter::SurfacePresenter;
pub use surface_session::{
    SurfaceAdaptiveState, SurfaceFrameOutput, SurfaceFrameTimings, SurfaceOrderBackend,
    SurfaceOrderBackendUsed, SurfaceRenderSession, SurfaceSortSchedule,
};

pub(crate) use math::CameraCovarianceTerms;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use offscreen::GpuRasterizer;
pub(crate) use resident::{
    GpuSurfaceRenderParams, ResidentSceneResources, create_resident_bind_group_layout,
    create_resident_pipeline,
};
pub(crate) use surface::{
    create_surface_instance, fit_surface_size, select_present_mode, surface_error_to_presenter,
};
pub(crate) use timing::{timer_elapsed_ms, timer_now, wgpu_label};

use gsplat_core::{Camera, FrameStats, RenderMode, RendererConfig, SceneBuffers};
use gsplat_sort::CpuSortBackend;

use crate::math::{precompute_alpha_values, precompute_world_covariances};
use crate::preprocess::preprocess_visible_into;
use crate::timing::TimerInstant;

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    cpu_sort_backend: CpuSortBackend,
    #[cfg(not(target_arch = "wasm32"))]
    gpu_rasterizer: Option<GpuRasterizer>,
    scene: Option<SceneBuffers>,
    world_covariances: Option<Vec<[[f32; 3]; 3]>>,
    pub(crate) world_covariance_terms: Option<Vec<CameraCovarianceTerms>>,
    pub(crate) alpha_values: Option<Vec<f32>>,
    preprocess_depth_keys: Vec<u32>,
    preprocess_indices: Vec<u32>,
    last_stats: FrameStats,
    storage_profile: ResidentStorageProfile,
    order_backend: SurfaceOrderBackend,
}

impl Renderer {
    pub fn new(mode: RenderMode) -> Result<Self, RendererError> {
        let config = RendererConfig {
            mode,
            ..RendererConfig::default()
        };
        Self::with_config(config)
    }

    pub fn with_config(config: RendererConfig) -> Result<Self, RendererError> {
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let gpu_rasterizer = GpuRasterizer::create(&config)?;
            let mut renderer = Self::from_validated_config(config);
            renderer.gpu_rasterizer = Some(gpu_rasterizer);
            Ok(renderer)
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    /// Create renderer state for a separate native or Web surface presenter.
    ///
    /// This constructor intentionally does not create the offscreen rasterizer
    /// used by [`Self::render_frame`] and [`Self::readback_rgba8`]. Surface
    /// clients render through [`SurfacePresenter`] instead.
    pub fn with_config_for_surface(config: RendererConfig) -> Result<Self, RendererError> {
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;
        Ok(Self::from_validated_config(config))
    }

    pub fn new_for_surface(mode: RenderMode) -> Result<Self, RendererError> {
        Self::with_config_for_surface(RendererConfig {
            mode,
            ..RendererConfig::default()
        })
    }

    fn from_validated_config(config: RendererConfig) -> Self {
        Self {
            mode: config.mode,
            config,
            cpu_sort_backend: CpuSortBackend::default(),
            #[cfg(not(target_arch = "wasm32"))]
            gpu_rasterizer: None,
            scene: None,
            world_covariances: None,
            world_covariance_terms: None,
            alpha_values: None,
            preprocess_depth_keys: Vec::new(),
            preprocess_indices: Vec::new(),
            last_stats: FrameStats::zero(),
            storage_profile: ResidentStorageProfile::FullF32,
            order_backend: SurfaceOrderBackend::Cpu,
        }
    }

    pub fn config(&self) -> RendererConfig {
        self.config
    }

    pub fn storage_profile(&self) -> ResidentStorageProfile {
        self.storage_profile
    }

    pub fn set_storage_profile(&mut self, profile: ResidentStorageProfile) {
        if self.storage_profile == profile {
            return;
        }
        self.storage_profile = profile;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
            rasterizer.clear_scene_resources();
        }
    }

    pub fn order_backend(&self) -> SurfaceOrderBackend {
        self.order_backend
    }

    /// Experimental offscreen order backend. `Adaptive` is Surface-only.
    /// GPU pipelines are created on the first GPU-order frame (warmup).
    pub fn set_order_backend(&mut self, backend: SurfaceOrderBackend) -> Result<(), RendererError> {
        if backend == SurfaceOrderBackend::Adaptive {
            return Err(RendererError::InvalidConfig);
        }
        self.order_backend = backend;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.gpu_rasterizer.as_ref().map(|gpu| &gpu.device)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.gpu_rasterizer.as_ref().map(|gpu| &gpu.queue)
    }

    pub fn set_size(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let config = RendererConfig {
            width,
            height,
            ..self.config
        };
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(gpu_rasterizer) = self.gpu_rasterizer.as_mut() {
            gpu_rasterizer.ensure_output_target(width, height)?;
        }

        self.config = config;
        Ok(())
    }

    pub fn mode(&self) -> RenderMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: RenderMode) {
        self.mode = mode;
        self.config.mode = mode;
    }

    pub fn has_gpu_rasterizer(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.gpu_rasterizer.is_some()
        }

        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn gpu_adapter_info(&self) -> Option<&wgpu::AdapterInfo> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.gpu_rasterizer
                .as_ref()
                .map(|rasterizer| &rasterizer.adapter_info)
        }

        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Reports whether the loaded scene fits the resident path on this renderer's
    /// effective offscreen device limits.
    ///
    /// Surface-only renderers do not own a device, so callers must query the
    /// presenter path separately instead of assuming adapter or default limits.
    pub fn current_resident_scene_preflight(
        &self,
    ) -> Result<ResidentScenePreflight, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            resident_scene_preflight_for_profile(
                scene.len(),
                scene.sh_degree,
                &rasterizer.device.limits(),
                self.storage_profile,
            )
            .map_err(RendererError::from)
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = scene;
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn wait_for_gpu(&self) -> Result<(), RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            rasterizer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| RendererError::GpuWait)?;
            Ok(())
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn ensure_resident_gpu_order_for_test(&mut self) -> Result<(), RendererError> {
        let rasterizer = self
            .gpu_rasterizer
            .as_mut()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        rasterizer.ensure_gpu_order().map_err(RendererError::from)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn render_frame_gpu_order_for_test(
        &mut self,
        camera: &Camera,
    ) -> Result<FrameStats, RendererError> {
        self.render_frame_gpu_order(camera)
    }

    pub fn load_scene(&mut self, scene: SceneBuffers) -> Result<(), RendererError> {
        scene.validate().map_err(|_| RendererError::InvalidScene)?;
        self.scene = Some(scene);
        self.rebuild_resident_cpu_data();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rasterizer) = self.gpu_rasterizer.as_mut() {
            rasterizer.clear_scene_resources();
        }
        Ok(())
    }

    fn rebuild_resident_cpu_data(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            self.world_covariances = None;
            self.world_covariance_terms = None;
            self.alpha_values = None;
            return;
        };
        let world_covariances = precompute_world_covariances(scene);
        let world_covariance_terms = world_covariances
            .iter()
            .copied()
            .map(CameraCovarianceTerms::from_matrix)
            .collect();
        let alpha_values = precompute_alpha_values(scene);
        self.world_covariances = Some(world_covariances);
        self.world_covariance_terms = Some(world_covariance_terms);
        self.alpha_values = Some(alpha_values);
    }

    pub fn scene(&self) -> Option<&SceneBuffers> {
        self.scene.as_ref()
    }

    pub fn world_covariances(&self) -> Option<&[[[f32; 3]; 3]]> {
        self.world_covariances.as_deref()
    }

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let mut output = PreprocessOutput {
            depth_keys: Vec::with_capacity(scene.len()),
            indices: Vec::with_capacity(scene.len()),
        };
        preprocess_visible_into(scene, camera, &mut output.depth_keys, &mut output.indices)?;
        Ok(output)
    }

    #[cfg(test)]
    pub(crate) fn build_sorted_instances(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<GpuInstance>, FrameStats), RendererError> {
        let mut instances = Vec::new();
        let stats = self.build_sorted_instances_into(camera, &mut instances)?;
        Ok((instances, stats))
    }

    fn preprocess_and_sort_timed(&mut self, camera: &Camera) -> Result<(f32, f32), RendererError> {
        let started = timer_now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = timer_elapsed_ms(started);
        let started = timer_now();
        self.sort_preprocessed_scratch()?;
        Ok((preprocess_ms, timer_elapsed_ms(started)))
    }

    fn record_stats(
        &mut self,
        frame_start: TimerInstant,
        preprocess_ms: f32,
        sort_ms: f32,
        raster_ms: f32,
        drawn_count: u32,
    ) -> FrameStats {
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: self.preprocess_indices.len() as u32,
            drawn_count,
        };
        self.last_stats = stats;
        stats
    }

    #[cfg(test)]
    pub(crate) fn build_sorted_instances_into(
        &mut self,
        camera: &Camera,
        instances: &mut Vec<GpuInstance>,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let raster_start = timer_now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let alpha_values = self
            .alpha_values
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        build_instances_into(
            scene,
            world_covariances,
            alpha_values,
            &self.preprocess_indices,
            camera,
            self.config,
            instances,
        );
        let drawn_count = instances.len() as u32;
        let raster_ms = timer_elapsed_ms(raster_start);

        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, raster_ms, drawn_count))
    }

    pub fn build_surface_sorted_indices_with_sort_refresh(
        &mut self,
        camera: &Camera,
        refresh_sort: bool,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let refresh_sort = refresh_sort || self.preprocess_indices.is_empty();
        let (preprocess_ms, sort_ms) = if refresh_sort {
            self.preprocess_and_sort_timed(camera)?
        } else {
            camera
                .validate()
                .map_err(|_| RendererError::InvalidCamera)?;
            (0.0, 0.0)
        };

        let drawn_count = self.preprocess_indices.len() as u32;
        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, 0.0, drawn_count))
    }

    pub fn current_sorted_indices(&self) -> &[u32] {
        &self.preprocess_indices
    }

    pub fn replace_surface_sorted_indices(
        &mut self,
        indices: Vec<u32>,
    ) -> Result<(), RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        if indices.iter().any(|&idx| idx as usize >= scene.len()) {
            return Err(RendererError::InvalidScene);
        }

        self.preprocess_depth_keys.clear();
        self.preprocess_indices = indices;
        Ok(())
    }

    pub fn build_sorted_indices(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<u32>, FrameStats), RendererError> {
        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let drawn_count = self.preprocess_indices.len() as u32;
        let stats = self.record_stats(frame_start, preprocess_ms, sort_ms, 0.0, drawn_count);
        Ok((self.preprocess_indices.clone(), stats))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn raster_sorted_indices(
        &mut self,
        camera: &Camera,
        sorted_indices: &[u32],
    ) -> Result<(), RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let rasterizer = self
            .gpu_rasterizer
            .as_mut()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        rasterizer.render_resident_sorted_indices(
            self.config,
            sorted_indices,
            camera,
            scene,
            self.world_covariance_terms
                .as_deref()
                .ok_or(RendererError::InvalidScene)?,
            self.alpha_values
                .as_deref()
                .ok_or(RendererError::InvalidScene)?,
            self.storage_profile,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        if self.order_backend == SurfaceOrderBackend::Gpu {
            return self.render_frame_gpu_order(camera);
        }
        if self.gpu_rasterizer.is_none() {
            return Err(RendererError::GpuRasterizerUnavailable);
        }

        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let raster_start = timer_now();
        let sorted_indices = std::mem::take(&mut self.preprocess_indices);
        let raster_result = self.raster_sorted_indices(camera, &sorted_indices);
        let drawn_count = sorted_indices.len() as u32;
        self.preprocess_indices = sorted_indices;
        raster_result?;
        let raster_ms = timer_elapsed_ms(raster_start);

        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, raster_ms, drawn_count))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_frame_gpu_order(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let rasterizer = self
            .gpu_rasterizer
            .as_mut()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        let frame_start = timer_now();
        let raster_start = timer_now();
        rasterizer.render_resident_gpu_order(
            self.config,
            camera,
            scene,
            self.world_covariance_terms
                .as_deref()
                .ok_or(RendererError::InvalidScene)?,
            self.alpha_values
                .as_deref()
                .ok_or(RendererError::InvalidScene)?,
            self.storage_profile,
        )?;
        let raster_ms = timer_elapsed_ms(raster_start);
        // GPU order compacts visibility on the GPU; the CPU cannot observe the
        // compacted count, so both counters report the resident source count.
        let count = u32::try_from(scene.len()).unwrap_or(u32::MAX);
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms,
            visible_count: count,
            drawn_count: count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn render_frame_with_external_order_for_test(
        &mut self,
        camera: &Camera,
        sorted_indices: &[u32],
    ) -> Result<FrameStats, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        let frame_start = timer_now();
        let raster_start = timer_now();
        self.raster_sorted_indices(camera, sorted_indices)?;
        let raster_ms = timer_elapsed_ms(raster_start);
        let count = u32::try_from(sorted_indices.len()).unwrap_or(u32::MAX);
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms,
            visible_count: count,
            drawn_count: count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn render_frame(&mut self, _camera: &Camera) -> Result<FrameStats, RendererError> {
        Err(RendererError::GpuRasterizerUnavailable)
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub fn readback_rgba8(&mut self) -> Result<Vec<u8>, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let rasterizer = self
                .gpu_rasterizer
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            rasterizer
                .readback_rgba8()
                .map_err(|_| RendererError::GpuReadback)
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn render_placeholder(&mut self) -> Result<FrameStats, RendererError> {
        self.render_frame(&Camera::default())
    }

    fn preprocess_visible_scratch(&mut self, camera: &Camera) -> Result<(), RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        preprocess_visible_into(
            scene,
            camera,
            &mut self.preprocess_depth_keys,
            &mut self.preprocess_indices,
        )
    }

    fn sort_preprocessed_scratch(&mut self) -> Result<(), RendererError> {
        self.cpu_sort_backend
            .sort_values_by_keys(&self.preprocess_depth_keys, &mut self.preprocess_indices)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, ErrorCode, RenderMode, RendererConfig, SceneBuffers, Vec3f};

    use super::{
        Renderer, RendererError, ResidentSceneError, ResidentScenePath, ResidentSceneRemediation,
        ResidentSceneResource, ResidentStorageProfile, resident_scene_preflight,
        resident_scene_preflight_for_profile,
    };
    use crate::cpu_geometry::{
        build_instances, ellipse_axes_from_covariance, project_covariance_to_ndc,
    };
    use crate::math::{precompute_world_covariances, quat_inverse};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::offscreen::offscreen_device_limits;

    fn build_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 0.5), Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![0.9, 0.8],
            scale_xyz: vec![[0.0, 0.0, 0.0], [0.2, 0.2, 0.2]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.1, 0.2, 0.3], [0.3, 0.2, 0.1]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    fn test_config(size: u32) -> RendererConfig {
        RendererConfig {
            width: size,
            height: size,
            mode: RenderMode::SortedAlpha,
        }
    }

    fn test_renderer(config: RendererConfig, label: &str) -> Option<Renderer> {
        match Renderer::with_config(config) {
            Ok(renderer) => Some(renderer),
            Err(super::RendererError::GpuRasterizerUnavailable)
            | Err(super::RendererError::GpuDeviceCreation) => {
                eprintln!("skipping {label}; adapter unavailable");
                None
            }
            Err(error) => panic!("renderer init: {error}"),
        }
    }

    #[test]
    fn sorted_alpha_pipeline_builds_visible_gaussians() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let (instances, stats) = renderer.build_sorted_instances(&Camera::default()).unwrap();

        assert_eq!(stats.visible_count, 2);
        assert_eq!(stats.drawn_count, 2);
        assert_eq!(instances.len(), 2);
    }

    #[derive(Debug, Clone, Copy)]
    struct ImageParityMetrics {
        mean_abs_rgb: f64,
        frac_pixels_over_3_255: f64,
        max_abs_rgb: f64,
    }

    fn rgba_image_parity_metrics(first: &[u8], second: &[u8]) -> ImageParityMetrics {
        assert_eq!(first.len(), second.len());
        assert_eq!(first.len() % 4, 0);
        let pixels = first.len() / 4;
        let mut sum = 0.0_f64;
        let mut pixels_over = 0_u64;
        let mut max_abs = 0.0_f64;
        for index in 0..pixels {
            let base = index * 4;
            let mut pixel_over = false;
            for channel in 0..3 {
                let a = f64::from(first[base + channel]) / 255.0;
                let b = f64::from(second[base + channel]) / 255.0;
                let err = (a - b).abs();
                sum += err;
                max_abs = max_abs.max(err);
                if err > 3.0 / 255.0 {
                    pixel_over = true;
                }
            }
            pixels_over += u64::from(pixel_over);
        }
        ImageParityMetrics {
            mean_abs_rgb: sum / ((pixels * 3) as f64).max(1.0),
            frac_pixels_over_3_255: (pixels_over as f64) / (pixels as f64).max(1.0),
            max_abs_rgb: max_abs,
        }
    }

    #[test]
    fn image_parity_threshold_counts_pixels_not_channels() {
        let first = [0_u8, 0, 0, 255, 0, 0, 0, 255];
        let second = [4_u8, 0, 0, 255, 0, 0, 0, 255];
        let metrics = rgba_image_parity_metrics(&first, &second);
        assert_eq!(metrics.frac_pixels_over_3_255, 0.5);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn orbit_camera_for_scene(scene: &SceneBuffers, config: RendererConfig, yaw: f32) -> Camera {
        let first = *scene.positions.first().expect("non-empty scene");
        let (mut min, mut max) = (first, first);
        for position in &scene.positions[1..] {
            min.x = min.x.min(position.x);
            min.y = min.y.min(position.y);
            min.z = min.z.min(position.z);
            max.x = max.x.max(position.x);
            max.y = max.y.max(position.y);
            max.z = max.z.max(position.z);
        }
        let center = Vec3f::new(
            (min.x + max.x) * 0.5,
            (min.y + max.y) * 0.5,
            (min.z + max.z) * 0.5,
        );
        let half_x = ((max.x - min.x) * 0.5).max(1e-3);
        let half_y = ((max.y - min.y) * 0.5).max(1e-3);
        let half_z = ((max.z - min.z) * 0.5).max(1e-3);
        let aspect = config.width as f32 / config.height.max(1) as f32;
        let vfov = Camera::default().intrinsics.vertical_fov_radians;
        let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();
        let distance =
            ((half_y / (vfov * 0.5).tan()).max(half_x / (hfov * 0.5).tan()) + half_z) * 1.2;
        let mut camera = Camera::default();
        camera.pose.position = Vec3f::new(
            center.x + yaw.sin() * distance,
            center.y,
            center.z - yaw.cos() * distance,
        );
        camera.pose.rotation_xyzw = [0.0, -(yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
        let radius = half_x.max(half_y).max(half_z);
        camera.intrinsics.near_plane = (distance - radius * 2.0).max(0.01);
        camera.intrinsics.far_plane = (distance + radius * 8.0).max(100.0);
        camera
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn assert_two_revision_stale_order_quality(scene: SceneBuffers, label: &str) {
        let config = test_config(128);
        let Some(mut reference) = test_renderer(config, "{label} stale-order quality") else {
            return;
        };
        let mut stale = Renderer::with_config(config).expect("second renderer");
        let old_camera = orbit_camera_for_scene(&scene, config, 0.0);
        let current_camera = orbit_camera_for_scene(&scene, config, 0.002);
        reference.load_scene(scene.clone()).unwrap();
        stale.load_scene(scene).unwrap();

        let (fresh_order, _) = reference.build_sorted_indices(&current_camera).unwrap();
        let (stale_order, _) = stale.build_sorted_indices(&old_camera).unwrap();
        let mut fresh_visible_set = fresh_order.clone();
        let mut stale_visible_set = stale_order.clone();
        fresh_visible_set.sort_unstable();
        stale_visible_set.sort_unstable();
        assert_eq!(
            fresh_visible_set, stale_visible_set,
            "{label} visible set changed across the two-revision quality envelope"
        );

        reference
            .render_frame_with_external_order_for_test(&current_camera, &fresh_order)
            .unwrap();
        stale
            .render_frame_with_external_order_for_test(&current_camera, &stale_order)
            .unwrap();
        let metrics = rgba_image_parity_metrics(
            &reference.readback_rgba8().unwrap(),
            &stale.readback_rgba8().unwrap(),
        );
        eprintln!(
            "{label} two-revision stale-order parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.frac_pixels_over_3_255, metrics.max_abs_rgb
        );
        assert!(metrics.mean_abs_rgb <= 1.0 / 255.0);
        assert!(metrics.frac_pixels_over_3_255 <= 0.001);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    #[ignore = "temporal stale-order pixel-tail gate is not yet met; retained as a research oracle"]
    fn bounded_async_two_revision_order_passes_kitsune_and_flowers_quality() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets");
        let datasets = [
            (
                "Kitsune",
                root.join("external/wakufactory_kitune/kitune1.ply"),
            ),
            (
                "Flowers",
                root.join("external/nvidia_flowers_1/flowers_1/flowers_1.ply"),
            ),
        ];
        for (label, path) in datasets {
            if !path.is_file() {
                eprintln!("skipping {label} stale-order quality; dataset missing");
                continue;
            }
            let loaded = gsplat_io_ply::load_ply(&path)
                .unwrap_or_else(|error| panic!("load {label} at {}: {error}", path.display()));
            assert_two_revision_stale_order_quality(loaded.scene, label);
        }
    }

    fn scene_to_rdf_ply_for_spz_parity(scene: &SceneBuffers) -> String {
        // Mirror gsplat-io-spz attribute-gate authoring: emit RDF so PLY load
        // recovers the same RUF SceneBuffers as the SPZ fixture.
        let mut ply =
            String::from("ply\nformat ascii 1.0\ncomment paired SPZ/PLY offscreen image parity\n");
        ply.push_str(&format!("element vertex {}\n", scene.len()));
        for property in [
            "x", "y", "z", "opacity", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2",
            "rot_3", "f_dc_0", "f_dc_1", "f_dc_2",
        ] {
            ply.push_str("property float ");
            ply.push_str(property);
            ply.push('\n');
        }
        ply.push_str("end_header\n");
        for index in 0..scene.len() {
            let position = scene.positions[index];
            let scale = scene.scale_xyz[index];
            let rotation = scene.rotation_xyzw[index];
            let color = scene.color_dc[index];
            let ply_w = rotation[3];
            let ply_x = -rotation[0];
            let ply_y = rotation[1];
            let ply_z = -rotation[2];
            ply.push_str(&format!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                position.x,
                -position.y,
                position.z,
                scene.opacity[index],
                scale[0],
                scale[1],
                scale[2],
                ply_w,
                ply_x,
                ply_y,
                ply_z,
                color[0],
                color[1],
                color[2],
            ));
        }
        ply
    }

    fn frame_camera_for_scene(scene: &SceneBuffers, width: u32, height: u32) -> Camera {
        let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for position in &scene.positions {
            min.x = min.x.min(position.x);
            min.y = min.y.min(position.y);
            min.z = min.z.min(position.z);
            max.x = max.x.max(position.x);
            max.y = max.y.max(position.y);
            max.z = max.z.max(position.z);
        }
        let center = Vec3f::new(
            0.5 * (min.x + max.x),
            0.5 * (min.y + max.y),
            0.5 * (min.z + max.z),
        );
        let half_x = ((max.x - min.x) * 0.5).max(1.0e-3);
        let half_y = ((max.y - min.y) * 0.5).max(1.0e-3);
        let half_z = ((max.z - min.z) * 0.5).max(1.0e-3);
        let aspect = width as f32 / height.max(1) as f32;
        let mut camera = Camera::default();
        let vfov = camera.intrinsics.vertical_fov_radians.max(1.0e-3);
        let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();
        let dist_y = half_y / (vfov * 0.5).tan();
        let dist_x = half_x / (hfov * 0.5).tan();
        let distance = (dist_y.max(dist_x) + half_z) * 1.2;
        let radius = half_x.max(half_y).max(half_z);
        camera.pose.position = Vec3f::new(center.x, center.y, center.z - distance);
        camera.intrinsics.near_plane = (distance - radius * 2.0).max(0.01);
        camera.intrinsics.far_plane = (distance + radius * 8.0).max(100.0);
        camera
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ply_vs_spz_offscreen_image_parity_gate_on_minimal_fixture() {
        let spz_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/datasets/minimal_v4_degree0.spz");
        let spz = gsplat_io_spz::load_spz(&spz_path).expect("load minimal SPZ fixture");
        let ply = gsplat_io_ply::parse_ply_text(&scene_to_rdf_ply_for_spz_parity(&spz.scene))
            .expect("paired RDF PLY must parse");
        assert_eq!(ply.summary.gaussians, spz.summary.gaussians);
        assert_eq!(ply.scene.len(), spz.scene.len());

        let config = test_config(128);
        let Some(mut from_spz) = test_renderer(config, "PLY-vs-SPZ image parity") else {
            return;
        };
        let mut from_ply = Renderer::with_config(config).expect("second renderer");
        from_spz.load_scene(spz.scene.clone()).unwrap();
        from_ply.load_scene(ply.scene).unwrap();
        let camera = frame_camera_for_scene(&spz.scene, config.width, config.height);
        let spz_ttff_started = std::time::Instant::now();
        let spz_stats = from_spz.render_frame(&camera).unwrap();
        let spz_ttff_ms = spz_ttff_started.elapsed().as_secs_f64() * 1_000.0;
        let ply_ttff_started = std::time::Instant::now();
        let ply_stats = from_ply.render_frame(&camera).unwrap();
        let ply_ttff_ms = ply_ttff_started.elapsed().as_secs_f64() * 1_000.0;
        assert!(
            spz_stats.visible_count > 0,
            "framed SPZ fixture must produce visible splats"
        );
        assert_eq!(spz_stats.visible_count, ply_stats.visible_count);
        assert_eq!(spz_stats.drawn_count, ply_stats.drawn_count);
        let metrics = rgba_image_parity_metrics(
            &from_spz.readback_rgba8().unwrap(),
            &from_ply.readback_rgba8().unwrap(),
        );
        eprintln!(
            "PLY-vs-SPZ minimal fixture parity: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6} visible={} spz_ttff_ms={:.3} ply_ttff_ms={:.3}",
            metrics.mean_abs_rgb,
            metrics.frac_pixels_over_3_255,
            metrics.max_abs_rgb,
            spz_stats.visible_count,
            spz_ttff_ms,
            ply_ttff_ms
        );
        assert!(
            metrics.mean_abs_rgb <= 1.0 / 255.0,
            "mean abs RGB {:.6} exceeded 1/255",
            metrics.mean_abs_rgb
        );
        assert!(
            metrics.frac_pixels_over_3_255 <= 0.001,
            "frac over 3/255 {:.6} exceeded 0.1%",
            metrics.frac_pixels_over_3_255
        );

        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/benchmarks/phase-c");
        std::fs::create_dir_all(&out_dir).unwrap();
        let out_path = out_dir.join("minimal-spz-vs-ply-ttff.json");
        let payload = format!(
            "{{\n  \"schema\": \"gsplat-phase-c-ttff/v1\",\n  \"dataset\": \"minimal_v4_degree0\",\n  \"width\": {},\n  \"height\": {},\n  \"visible\": {},\n  \"drawn\": {},\n  \"ttff_ms\": {{\n    \"spz_first_frame\": {:.6},\n    \"ply_first_frame\": {:.6}\n  }},\n  \"notes\": \"ttff_ms measures first SortedAlpha render_frame after load_scene; adapter-dependent\"\n}}\n",
            config.width,
            config.height,
            spz_stats.visible_count,
            spz_stats.drawn_count,
            spz_ttff_ms,
            ply_ttff_ms,
        );
        std::fs::write(&out_path, payload).unwrap();
        eprintln!("wrote {}", out_path.display());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quantized_profile_renders_close_to_full_f32() {
        let config = test_config(64);
        let Some(mut reference) = test_renderer(config, "quantized vs f32") else {
            return;
        };
        let mut quantized = Renderer::with_config(config).expect("second renderer");
        quantized.set_storage_profile(ResidentStorageProfile::Quantized);
        let scene = build_scene();
        reference.load_scene(scene.clone()).unwrap();
        quantized.load_scene(scene).unwrap();
        let camera = Camera::default();
        reference.render_frame(&camera).unwrap();
        quantized.render_frame(&camera).unwrap();
        let metrics = rgba_image_parity_metrics(
            &reference.readback_rgba8().unwrap(),
            &quantized.readback_rgba8().unwrap(),
        );
        eprintln!(
            "quantized vs f32: mean_abs_rgb={:.6} frac_over_3_255={:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.frac_pixels_over_3_255, metrics.max_abs_rgb
        );
        assert!(
            metrics.mean_abs_rgb <= 2.0 / 255.0,
            "quantized mean abs RGB {:.6} exceeded 2/255",
            metrics.mean_abs_rgb
        );
    }

    fn ssim_luma_srgb_window8(first: &[u8], second: &[u8], width: u32, height: u32) -> f64 {
        assert_eq!(first.len(), second.len());
        assert_eq!(first.len(), (width * height * 4) as usize);
        const WINDOW: u32 = 8;
        const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
        const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
        let mut scores = Vec::new();
        let mut top = 0_u32;
        while top < height {
            let mut left = 0_u32;
            while left < width {
                let bottom = (top + WINDOW).min(height);
                let right = (left + WINDOW).min(width);
                let mut luma_a = Vec::new();
                let mut luma_b = Vec::new();
                for y in top..bottom {
                    for x in left..right {
                        let offset = ((y * width + x) * 4) as usize;
                        luma_a.push(
                            0.2126 * f64::from(first[offset])
                                + 0.7152 * f64::from(first[offset + 1])
                                + 0.0722 * f64::from(first[offset + 2]),
                        );
                        luma_b.push(
                            0.2126 * f64::from(second[offset])
                                + 0.7152 * f64::from(second[offset + 1])
                                + 0.0722 * f64::from(second[offset + 2]),
                        );
                    }
                }
                scores.push(window_ssim(&luma_a, &luma_b, C1, C2));
                left += WINDOW;
            }
            top += WINDOW;
        }
        scores.iter().sum::<f64>() / scores.len().max(1) as f64
    }

    fn window_ssim(a: &[f64], b: &[f64], c1: f64, c2: f64) -> f64 {
        let count = a.len() as f64;
        let mean_a = a.iter().sum::<f64>() / count;
        let mean_b = b.iter().sum::<f64>() / count;
        let mut var_a = 0.0;
        let mut var_b = 0.0;
        let mut cov = 0.0;
        for index in 0..a.len() {
            let da = a[index] - mean_a;
            let db = b[index] - mean_b;
            var_a += da * da;
            var_b += db * db;
            cov += da * db;
        }
        let denom = (count - 1.0).max(1.0);
        var_a /= denom;
        var_b /= denom;
        cov /= denom;
        ((2.0 * mean_a * mean_b + c1) * (2.0 * cov + c2))
            / ((mean_a * mean_a + mean_b * mean_b + c1) * (var_a + var_b + c2))
    }

    fn build_degree_three_scene() -> SceneBuffers {
        let mut rest = [0.0_f32; 45];
        rest[0] = 0.2;
        rest[1] = -0.15;
        rest[8] = 0.1;
        rest[15] = 0.05;
        rest[30] = -0.08;
        SceneBuffers {
            positions: vec![
                Vec3f::new(0.0, 0.0, 1.5),
                Vec3f::new(0.15, 0.0, 1.8),
                Vec3f::new(-0.12, 0.08, 1.4),
            ],
            opacity: vec![0.95, 0.9, 0.85],
            scale_xyz: vec![[-1.2, -1.4, -1.5], [-1.1, -1.1, -1.3], [-1.3, -1.2, -1.4]],
            rotation_xyzw: vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.1, 0.0, 0.995],
                [0.05, 0.0, 0.05, 0.997],
            ],
            color_dc: vec![[0.4, 0.2, 0.1], [0.1, 0.35, 0.2], [0.2, 0.15, 0.4]],
            sh_degree: 3,
            sh_rest: Some(rest.as_slice().repeat(3)),
        }
    }

    #[test]
    fn ssim_self_check_identical_is_one() {
        let pixels = [
            0_u8, 64, 128, 255, 10, 20, 30, 255, 255, 128, 0, 255, 1, 2, 3, 255,
        ];
        let score = ssim_luma_srgb_window8(&pixels, &pixels, 2, 2);
        assert!((score - 1.0).abs() < 1e-12, "{score}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_profile_pair(
        scene: SceneBuffers,
        size: u32,
        label: &str,
        camera: &Camera,
    ) -> Option<(Vec<u8>, Vec<u8>, f32, f32)> {
        let config = test_config(size);
        let mut reference = test_renderer(config, label)?;
        let mut quantized = Renderer::with_config(config).expect("second renderer");
        quantized.set_storage_profile(ResidentStorageProfile::Quantized);
        reference.load_scene(scene.clone()).unwrap();
        quantized.load_scene(scene).unwrap();
        let started = std::time::Instant::now();
        reference.render_frame(camera).unwrap();
        let f32_ms = started.elapsed().as_secs_f32() * 1000.0;
        let started = std::time::Instant::now();
        quantized.render_frame(camera).unwrap();
        let quantized_ms = started.elapsed().as_secs_f32() * 1000.0;
        Some((
            reference.readback_rgba8().unwrap(),
            quantized.readback_rgba8().unwrap(),
            f32_ms,
            quantized_ms,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quantized_degree_three_matches_f32_ssim() {
        let scene = build_degree_three_scene();
        scene.validate().unwrap();
        let Some((reference, quantized, f32_ms, quantized_ms)) =
            render_profile_pair(scene, 64, "quantized degree-3 SSIM", &Camera::default())
        else {
            return;
        };
        let metrics = rgba_image_parity_metrics(&reference, &quantized);
        let ssim = ssim_luma_srgb_window8(&reference, &quantized, 64, 64);
        eprintln!(
            "quantized degree-3: mean_abs_rgb={:.6} ssim={:.6} first_frame_ms f32={:.3} quantized={:.3}",
            metrics.mean_abs_rgb, ssim, f32_ms, quantized_ms
        );
        assert!(ssim >= 0.99, "degree-3 SSIM {ssim} below 0.99");
        assert!(metrics.mean_abs_rgb <= 3.0 / 255.0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quantized_gpu_order_initializes_after_first_frame() {
        let config = test_config(32);
        let Some(mut renderer) = test_renderer(config, "quantized GPU order") else {
            return;
        };
        renderer.set_storage_profile(ResidentStorageProfile::Quantized);
        renderer.load_scene(build_degree_three_scene()).unwrap();
        renderer.render_frame(&Camera::default()).unwrap();
        renderer
            .ensure_resident_gpu_order_for_test()
            .expect("quantized GPU order should initialize");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn assert_cpu_gpu_order_image_parity(
        scene: SceneBuffers,
        camera: &Camera,
        size: u32,
        label: &str,
        profile: ResidentStorageProfile,
    ) {
        let Some(mut renderer) = test_renderer(test_config(size), label) else {
            return;
        };
        renderer.set_storage_profile(profile);
        renderer.load_scene(scene).unwrap();
        renderer.render_frame(camera).unwrap();
        let cpu = renderer.readback_rgba8().unwrap();
        renderer
            .render_frame_gpu_order_for_test(camera)
            .unwrap_or_else(|error| panic!("{label} GPU order render: {error}"));
        let gpu = renderer.readback_rgba8().unwrap();
        let metrics = rgba_image_parity_metrics(&cpu, &gpu);
        let ssim = ssim_luma_srgb_window8(&cpu, &gpu, size, size);
        eprintln!(
            "{label} ({profile:?}): mean_abs_rgb={:.6} ssim={ssim:.6} max_abs_rgb={:.6}",
            metrics.mean_abs_rgb, metrics.max_abs_rgb
        );
        assert!(
            ssim >= 0.99,
            "{label} CPU vs GPU-order SSIM {ssim} below 0.99"
        );
        assert!(
            metrics.mean_abs_rgb <= 2.0 / 255.0,
            "{label} mean abs RGB {:.6} exceeded 2/255",
            metrics.mean_abs_rgb
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn empty_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![],
            opacity: vec![],
            scale_xyz: vec![],
            rotation_xyzw: vec![],
            color_dc: vec![],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn near_far_mixed_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![
                Vec3f::new(0.0, 0.0, -1.0),
                Vec3f::new(0.0, 0.0, 1.5),
                Vec3f::new(0.0, 0.0, 50.0),
            ],
            opacity: vec![0.9, 0.95, 0.8],
            scale_xyz: vec![[-1.2, -1.2, -1.2], [-1.1, -1.3, -1.2], [-1.0, -1.0, -1.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 3],
            color_dc: vec![[0.6, 0.1, 0.1], [0.1, 0.5, 0.2], [0.2, 0.2, 0.6]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn screen_edge_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.5), Vec3f::new(20.0, 0.0, 1.5)],
            opacity: vec![0.95, 0.95],
            scale_xyz: vec![[-1.2, -1.2, -1.2], [-1.2, -1.2, -1.2]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.2, 0.5, 0.3], [0.8, 0.1, 0.1]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn duplicate_depth_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(-0.08, 0.0, 1.5), Vec3f::new(0.08, 0.0, 1.5)],
            opacity: vec![0.85, 0.85],
            scale_xyz: vec![[-1.0, -1.0, -1.0], [-1.0, -1.0, -1.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.7, 0.1, 0.1], [0.1, 0.1, 0.7]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn degenerate_covariance_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.5)],
            opacity: vec![0.9],
            scale_xyz: vec![[0.0, 0.0, 0.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.3, 0.4, 0.2]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gpu_order_matches_cpu_image_for_empty_near_far_edge_ties_and_degenerate() {
        let camera = Camera::default();
        let mut near_far_camera = camera;
        near_far_camera.intrinsics.far_plane = 10.0;
        let cases = [
            (empty_scene(), camera, "empty"),
            (near_far_mixed_scene(), near_far_camera, "near-far"),
            (screen_edge_scene(), camera, "screen-edge"),
            (duplicate_depth_scene(), camera, "duplicate-depth"),
            (degenerate_covariance_scene(), camera, "degenerate-cov"),
            (build_degree_three_scene(), camera, "degree-3"),
        ];
        for (scene, camera, label) in cases {
            scene.validate().unwrap();
            assert_cpu_gpu_order_image_parity(
                scene,
                &camera,
                64,
                label,
                ResidentStorageProfile::FullF32,
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quantized_gpu_order_matches_quantized_cpu_image() {
        assert_cpu_gpu_order_image_parity(
            build_degree_three_scene(),
            &Camera::default(),
            64,
            "quantized degree-3",
            ResidentStorageProfile::Quantized,
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gpu_order_matches_cpu_image_on_real_scenes_when_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets");
        let datasets = [
            (
                "Kitsune",
                root.join("external/wakufactory_kitune/kitune1.ply"),
            ),
            (
                "Flowers",
                root.join("external/nvidia_flowers_1/flowers_1/flowers_1.ply"),
            ),
        ];
        let mut ran = 0_u32;
        for (label, path) in datasets {
            if !path.is_file() {
                eprintln!("skipping {label} GPU-order parity; dataset missing");
                continue;
            }
            let loaded = gsplat_io_ply::load_ply(&path)
                .unwrap_or_else(|error| panic!("load {label} at {}: {error}", path.display()));
            let camera = orbit_camera_for_scene(&loaded.scene, test_config(128), 0.0);
            assert_cpu_gpu_order_image_parity(
                loaded.scene,
                &camera,
                128,
                label,
                ResidentStorageProfile::FullF32,
            );
            ran += 1;
        }
        if ran == 0 {
            eprintln!("no local real scenes available for GPU-order parity");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quantized_real_scenes_keep_high_ssim_when_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/datasets");
        let datasets = [
            (
                "Kitsune",
                root.join("external/wakufactory_kitune/kitune1.ply"),
            ),
            (
                "Flowers",
                root.join("external/nvidia_flowers_1/flowers_1/flowers_1.ply"),
            ),
        ];
        let mut ran = 0_u32;
        for (label, path) in datasets {
            if !path.is_file() {
                eprintln!("skipping {label} quantized SSIM; dataset missing");
                continue;
            }
            let loaded = gsplat_io_ply::load_ply(&path)
                .unwrap_or_else(|error| panic!("load {label} at {}: {error}", path.display()));
            let camera = orbit_camera_for_scene(&loaded.scene, test_config(128), 0.0);
            let Some((reference, quantized, f32_ms, quantized_ms)) = render_profile_pair(
                loaded.scene,
                128,
                &format!("{label} quantized SSIM"),
                &camera,
            ) else {
                return;
            };
            let metrics = rgba_image_parity_metrics(&reference, &quantized);
            let ssim = ssim_luma_srgb_window8(&reference, &quantized, 128, 128);
            eprintln!(
                "{label} quantized vs f32: splats-ssim={ssim:.6} mean_abs_rgb={:.6} first_frame_ms f32={f32_ms:.3} quantized={quantized_ms:.3}",
                metrics.mean_abs_rgb
            );
            assert!(ssim >= 0.99, "{label} SSIM {ssim} below 0.99");
            ran += 1;
        }
        if ran == 0 {
            eprintln!("no local real scenes available for quantized SSIM");
        }
    }

    #[test]
    fn sorted_alpha_orders_visible_indices_back_to_front() {
        let scene = SceneBuffers {
            positions: vec![
                Vec3f::new(0.0, 0.0, 0.5),
                Vec3f::new(0.0, 0.0, 2.0),
                Vec3f::new(0.0, 0.0, 1.0),
                Vec3f::new(0.0, 0.0, -1.0),
                Vec3f::new(0.0, 0.0, 2000.0),
            ],
            opacity: vec![1.0; 5],
            scale_xyz: vec![[0.0, 0.0, 0.0]; 5],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 5],
            color_dc: vec![[0.2, 0.3, 0.4]; 5],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(scene).unwrap();

        let stats = renderer
            .build_surface_sorted_indices_with_sort_refresh(&Camera::default(), true)
            .unwrap();

        assert_eq!(stats.visible_count, 3);
        assert_eq!(stats.drawn_count, 3);
        assert_eq!(renderer.current_sorted_indices(), &[1, 2, 0]);
    }

    #[test]
    fn preprocess_rejects_missing_scene() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        let err = renderer.preprocess_visible(&Camera::default()).unwrap_err();
        assert_eq!(
            err.code() as i32,
            gsplat_core::ErrorCode::SceneNotLoaded as i32
        );
    }

    #[test]
    fn preprocess_rejects_invalid_camera() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();
        let mut camera = Camera::default();
        camera.intrinsics.vertical_fov_radians = 0.0;

        let err = renderer.preprocess_visible(&camera).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[test]
    fn quaternion_inverse_normalizes_scaled_input() {
        assert_eq!(quat_inverse([0.0, 0.0, 0.0, 2.0]), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn surface_renderer_constructs_without_offscreen_gpu() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        assert!(!renderer.has_gpu_rasterizer());
    }

    #[test]
    fn resident_scene_preflight_accessor_requires_a_loaded_scene() {
        let renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();

        let error = renderer.current_resident_scene_preflight().unwrap_err();

        assert!(matches!(error, super::RendererError::SceneNotLoaded));
    }

    #[test]
    fn resident_scene_preflight_accessor_does_not_guess_surface_device_limits() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let error = renderer.current_resident_scene_preflight().unwrap_err();

        assert!(matches!(
            error,
            super::RendererError::GpuRasterizerUnavailable
        ));
    }

    #[test]
    fn surface_renderer_rejects_offscreen_render() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let err = renderer.render_frame(&Camera::default()).unwrap_err();

        assert!(matches!(
            err,
            super::RendererError::GpuRasterizerUnavailable
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_reject_unsupported_dimensions_before_device_creation() {
        let adapter_limits = wgpu::Limits::downlevel_defaults();
        let config = RendererConfig {
            width: 4096,
            height: 2160,
            mode: RenderMode::SortedAlpha,
        };

        let err = offscreen_device_limits(&config, &adapter_limits).unwrap_err();

        assert!(matches!(
            err,
            RendererError::GpuDimensionsUnsupported {
                width: 4096,
                height: 2160,
                max_dimension: 2048,
            }
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_preserve_adapter_texture_dimension_for_later_resident_scenes() {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_texture_dimension_2d = 8192;
        let config = RendererConfig {
            width: 4096,
            height: 2160,
            mode: RenderMode::SortedAlpha,
        };

        let requested = offscreen_device_limits(&config, &adapter_limits).unwrap();

        assert_eq!(requested.max_texture_dimension_2d, 8192);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_limits_raise_storage_buffers_per_stage_when_adapter_allows() {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_storage_buffers_per_shader_stage = 8;
        adapter_limits.max_texture_dimension_2d = 8192;
        let config = RendererConfig {
            width: 64,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };

        let requested = offscreen_device_limits(&config, &adapter_limits).unwrap();

        assert_eq!(requested.max_storage_buffers_per_shader_stage, 8);
    }

    fn limits_with_storage_binding_limit(bytes: u32) -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffer_binding_size = bytes;
        limits.max_buffer_size = u64::from(bytes);
        limits.max_storage_buffers_per_shader_stage = 8;
        limits
    }

    #[test]
    fn resident_scene_preflight_accounts_for_empty_scene_fallback_buffers() {
        let report =
            resident_scene_preflight(0, 0, &limits_with_storage_binding_limit(128 * 1024 * 1024))
                .unwrap();

        assert_eq!(report.path, ResidentScenePath::Resident);
        assert_eq!(report.requirements[0].required_bytes, 4);
        assert_eq!(report.requirements[1].required_bytes, 64);
        assert_eq!(report.requirements[2].required_bytes, 4);
        assert_eq!(report.requirements[3].required_bytes, 48);
    }

    #[test]
    fn resident_scene_preflight_enforces_source_binding_boundary() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let at_limit = resident_scene_preflight(2_097_152, 0, &limits).unwrap();
        let above_limit = resident_scene_preflight(2_097_153, 0, &limits).unwrap();

        assert_eq!(at_limit.path, ResidentScenePath::Resident);
        assert_eq!(at_limit.limiting_resource, ResidentSceneResource::Source);
        assert_eq!(above_limit.path, ResidentScenePath::CapacityExceeded);
        assert_eq!(above_limit.requirements[1].required_bytes, 134_217_792);
        assert_eq!(
            above_limit.remediation,
            ResidentSceneRemediation::ReduceScene {
                max_resident_splats: 2_097_152,
            }
        );
    }

    #[test]
    fn resident_scene_preflight_enforces_degree_three_sh_boundary() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let at_limit = resident_scene_preflight(745_654, 3, &limits).unwrap();
        let above_limit = resident_scene_preflight(745_655, 3, &limits).unwrap();

        assert_eq!(at_limit.path, ResidentScenePath::Resident);
        assert_eq!(at_limit.limiting_resource, ResidentSceneResource::ShRest);
        assert_eq!(above_limit.path, ResidentScenePath::CapacityExceeded);
        assert_eq!(above_limit.requirements[2].required_bytes, 134_217_900);
    }

    #[test]
    fn quantized_degree_three_fits_one_million_in_128_mib() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let at_limit = resident_scene_preflight_for_profile(
            1_000_000,
            3,
            &limits,
            ResidentStorageProfile::Quantized,
        )
        .unwrap();
        let above_projected = resident_scene_preflight_for_profile(
            2_796_203,
            3,
            &limits,
            ResidentStorageProfile::Quantized,
        )
        .unwrap();

        assert_eq!(at_limit.path, ResidentScenePath::Resident);
        assert_eq!(at_limit.requirements[2].required_bytes, 21_000_000);
        assert_eq!(above_projected.path, ResidentScenePath::CapacityExceeded);
        assert_eq!(
            above_projected.limiting_resource,
            ResidentSceneResource::Projected
        );
    }

    #[test]
    fn quantized_preflight_rejects_downlevel_storage_buffer_count() {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffers_per_shader_stage = 4;
        let error = resident_scene_preflight_for_profile(
            1_000,
            3,
            &limits,
            ResidentStorageProfile::Quantized,
        )
        .unwrap_err();
        assert_eq!(
            error,
            ResidentSceneError::StorageBuffersPerStage {
                required: 7,
                available: 4,
            }
        );
    }

    #[test]
    fn resident_scene_preflight_reports_nandi_without_allocating_scene_data() {
        let limits = limits_with_storage_binding_limit(128 * 1024 * 1024);
        let dc = resident_scene_preflight(3_454_040, 0, &limits).unwrap();
        let degree_three = resident_scene_preflight(3_454_040, 3, &limits).unwrap();

        assert_eq!(dc.path, ResidentScenePath::CapacityExceeded);
        assert_eq!(dc.limiting_resource, ResidentSceneResource::Source);
        assert_eq!(dc.requirements[1].required_bytes, 221_058_560);
        assert_eq!(degree_three.path, ResidentScenePath::CapacityExceeded);
        assert_eq!(
            degree_three.limiting_resource,
            ResidentSceneResource::ShRest
        );
        assert_eq!(degree_three.requirements[2].required_bytes, 621_727_200);
        assert!(!degree_three.requirements[1].fits);
        assert!(!degree_three.requirements[2].fits);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn resident_scene_preflight_rejects_more_than_u32_draw_instances() {
        let count = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
        let report = resident_scene_preflight(count, 0, &wgpu::Limits::default()).unwrap();

        assert_eq!(report.path, ResidentScenePath::CapacityExceeded);
        assert!(report.splat_count > u64::from(u32::MAX));
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn resident_scene_preflight_rejects_byte_arithmetic_overflow() {
        let error = resident_scene_preflight(usize::MAX, 3, &wgpu::Limits::default()).unwrap_err();

        assert_eq!(error, ResidentSceneError::ResourceSizeOverflow);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn four_k_offscreen_construction_never_unwinds() {
        let config = RendererConfig {
            width: 4096,
            height: 64,
            mode: RenderMode::SortedAlpha,
        };

        let result = std::panic::catch_unwind(|| Renderer::with_config(config));

        assert!(result.is_ok(), "4K construction must return a Result");
        if let Err(error) = result.unwrap() {
            assert!(matches!(
                error,
                super::RendererError::GpuRasterizerUnavailable
                    | super::RendererError::GpuDeviceCreation
                    | super::RendererError::GpuDimensionsUnsupported { .. }
            ));
        }
    }

    #[test]
    fn covariance_projection_produces_nonzero_ellipse_axes() {
        let camera = Camera::default();
        let config = RendererConfig::default();
        let cov_cam = [
            [0.020, 0.000, 0.000],
            [0.000, 0.005, 0.000],
            [0.000, 0.000, 0.002],
        ];
        let cov2 = project_covariance_to_ndc(Vec3f::new(0.2, -0.1, 2.0), cov_cam, &camera, config)
            .expect("covariance should project");
        let (axis_u, axis_v) =
            ellipse_axes_from_covariance(cov2).expect("ellipse axes should be finite");

        let lu = (axis_u[0] * axis_u[0] + axis_u[1] * axis_u[1]).sqrt();
        let lv = (axis_v[0] * axis_v[0] + axis_v[1] * axis_v[1]).sqrt();
        assert!(lu > 0.0);
        assert!(lv > 0.0);
        assert!(lu > lv);
    }

    #[test]
    fn build_instances_generates_anisotropic_oriented_axes() {
        let qz = 0.5_f32.sqrt(); // sin/cos(90deg / 2)
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[0.8, -0.4, -0.4]],
            rotation_xyzw: vec![[0.0, 0.0, qz, qz]],
            color_dc: vec![[0.2, 0.3, 0.4]],
            sh_degree: 0,
            sh_rest: None,
        };

        let world_cov = precompute_world_covariances(&scene);
        let alpha_values = super::precompute_alpha_values(&scene);
        let instances = build_instances(
            &scene,
            &world_cov,
            &alpha_values,
            &[0],
            &Camera::default(),
            RendererConfig::default(),
        );
        assert_eq!(instances.len(), 1);
        let inst = instances[0];
        let axis_u = [inst.center_and_axis_u[2], inst.center_and_axis_u[3]];
        let axis_v = [inst.axis_v_and_pad[0], inst.axis_v_and_pad[1]];

        let lu = (axis_u[0] * axis_u[0] + axis_u[1] * axis_u[1]).sqrt();
        let lv = (axis_v[0] * axis_v[0] + axis_v[1] * axis_v[1]).sqrt();
        let dot = axis_u[0] * axis_v[0] + axis_u[1] * axis_v[1];
        assert!(lu > lv);
        assert!(dot.abs() < 1e-4);
    }

    #[test]
    fn build_instances_keeps_partial_splats_with_offscreen_center() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(2.3, 0.0, 1.0)],
            opacity: vec![1.0],
            // Large sigma to ensure the projected ellipse overlaps the viewport.
            scale_xyz: vec![[2.0, 2.0, 2.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.2, 0.3, 0.4]],
            sh_degree: 0,
            sh_rest: None,
        };

        let world_cov = precompute_world_covariances(&scene);
        let alpha_values = super::precompute_alpha_values(&scene);
        let instances = build_instances(
            &scene,
            &world_cov,
            &alpha_values,
            &[0],
            &Camera::default(),
            RendererConfig::default(),
        );
        assert_eq!(instances.len(), 1);

        let inst = instances[0];
        let center_x = inst.center_and_axis_u[0];
        let extent_x = inst.center_and_axis_u[2].abs() + inst.axis_v_and_pad[0].abs();
        assert!(center_x > 2.0);
        assert!(center_x - extent_x <= 1.0);
    }
}
