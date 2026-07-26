//! Public Renderer semantic facade.
//!
//! This private module contains inherent methods on the crate-root `Renderer`.
//! The facade remains the sole owner of public API, scene/path transactions,
//! Exact runtime publication, CPU order selection, and final frame statistics.

use gsplat_core::{Camera, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};

use crate::cpu::reference::{
    build_instances_into, log_scale_has_finite_nonzero_covariance, rotation_has_finite_nonzero_norm,
};
use crate::cpu_order::CpuOrderEngine;
use crate::data::{CameraCovarianceTerms, CpuPositionView};
use crate::renderer;
use crate::{
    DirectScenePreflight, GeometryPath, GpuInstance, PackedScenePreflight, PreprocessOutput,
    Renderer, RendererError, ResidentSceneCpu, SpatialPageSet, SurfacePresenterError, TimerInstant,
    map_prepared_gpu_runtime_error, plans, preprocess_positions_visible_into, timer_elapsed_ms,
    timer_now,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{
    RENDER_TARGET_FORMAT, direct_scene_preflight, map_frame_execution_error,
    packed_scene_preflight_with_limits,
};

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
            let offscreen_host = renderer::offscreen_host::OffscreenHost::create(&config)?;
            let mut renderer = Self::from_validated_config(config);
            renderer.offscreen_host = Some(offscreen_host);
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
    /// clients render through [`crate::SurfacePresenter`] instead.
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
            geometry_path: GeometryPath::SortedIndexDirect,
            cpu_order_engine: CpuOrderEngine::default(),
            #[cfg(not(target_arch = "wasm32"))]
            offscreen_host: None,
            scene_state: renderer::scene_state::RendererSceneState::empty(),
            exact_offscreen_runtime: None,
            surface_attempt_order: None,
            surface_attempt_stats: None,
            last_stats: FrameStats::zero(),
        }
    }

    pub fn config(&self) -> RendererConfig {
        self.config
    }

    pub fn geometry_path(&self) -> GeometryPath {
        self.geometry_path
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.offscreen_host
            .as_ref()
            .map(|host| host.device().as_ref())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.offscreen_host
            .as_ref()
            .map(|host| host.queue().as_ref())
    }

    pub fn set_geometry_path(&mut self, path: GeometryPath) {
        if self.geometry_path != path {
            self.discard_surface_attempt();
            self.geometry_path = path;
            self.scene_state.rebuild_for_path(path);
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(host) = self.offscreen_host.as_mut() {
                host.clear_scene_resources();
            }
        }
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
        if let Some(host) = self.offscreen_host.as_mut() {
            host.ensure_output_target(width, height)?;
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
            self.offscreen_host.is_some()
        }

        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn gpu_adapter_info(&self) -> Option<&wgpu::AdapterInfo> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.offscreen_host
                .as_ref()
                .map(renderer::offscreen_host::OffscreenHost::adapter_info)
        }

        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Reports whether the loaded scene fits the Direct path on this renderer's
    /// effective offscreen device limits.
    ///
    /// Surface-only renderers do not own a device, so callers must query the
    /// presenter path separately instead of assuming adapter or default limits.
    pub fn current_direct_scene_preflight(&self) -> Result<DirectScenePreflight, RendererError> {
        let scene = self
            .scene_state
            .wide()
            .ok_or(RendererError::SceneNotLoaded)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let host = self
                .offscreen_host
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            direct_scene_preflight(scene.len(), scene.sh_degree, &host.device().limits())
                .map_err(RendererError::from)
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = scene;
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    /// Reports whether the loaded scene fits the complete resident Packed path
    /// on this renderer's effective offscreen device limits.
    ///
    /// Like [`Self::current_direct_scene_preflight`], a Surface-only renderer
    /// has no device limits to report and returns
    /// [`RendererError::GpuRasterizerUnavailable`] instead of guessing.
    pub fn current_packed_scene_preflight(&self) -> Result<PackedScenePreflight, RendererError> {
        let scene_len = self.scene_len().ok_or(RendererError::SceneNotLoaded)?;
        let sh_degree = self
            .scene_sh_degree()
            .ok_or(RendererError::SceneNotLoaded)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let host = self
                .offscreen_host
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            packed_scene_preflight_with_limits(scene_len, sh_degree, &host.device().limits())
                .map_err(RendererError::from)
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = (scene_len, sh_degree);
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn wait_for_gpu(&self) -> Result<(), RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let host = self
                .offscreen_host
                .as_ref()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            host.device()
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| RendererError::GpuWait)?;
            Ok(())
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(RendererError::GpuRasterizerUnavailable)
        }
    }

    pub fn load_scene(&mut self, scene: SceneBuffers) -> Result<(), RendererError> {
        scene.validate().map_err(|_| RendererError::InvalidScene)?;
        if self.geometry_path == GeometryPath::PackedAtlas {
            // Encode before mutating self so a failed allocation/validation
            // leaves the previously loaded scene intact.
            let resident = ResidentSceneCpu::encode_owned(scene)?;
            return self.load_resident_scene(resident);
        }
        if !scene
            .scale_xyz
            .iter()
            .copied()
            .all(log_scale_has_finite_nonzero_covariance)
            || !scene
                .rotation_xyzw
                .iter()
                .copied()
                .all(rotation_has_finite_nonzero_norm)
        {
            return Err(RendererError::InvalidScene);
        }

        self.scene_state.replace_wide(scene, self.geometry_path);
        self.discard_surface_attempt();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.exact_offscreen_runtime = None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(host) = self.offscreen_host.as_mut() {
            host.clear_scene_resources();
        }
        Ok(())
    }

    /// Transactionally publish an already encoded exact resident scene.
    ///
    /// The input is fully validated before any renderer state is changed. This
    /// entrypoint is intentionally limited to [`GeometryPath::PackedAtlas`];
    /// Direct and Paged require their wide source attributes.
    pub fn load_resident_scene(&mut self, resident: ResidentSceneCpu) -> Result<(), RendererError> {
        resident.validate_complete()?;
        if self.geometry_path != GeometryPath::PackedAtlas {
            return Err(RendererError::InvalidScene);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(host) = self.offscreen_host.as_ref() {
            let candidate = pollster::block_on(
                renderer::PreparedRuntimeSlot::prepare_complete_gpu_candidate(
                    resident,
                    self.exact_offscreen_runtime.as_ref(),
                    host.device(),
                    host.queue(),
                    RENDER_TARGET_FORMAT,
                ),
            )
            .map_err(map_prepared_gpu_runtime_error)?;
            self.publish_exact_offscreen_candidate(candidate);
            return Ok(());
        }

        self.scene_state.replace_resident(resident);
        self.discard_surface_attempt();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(host) = self.offscreen_host.as_mut() {
            host.clear_scene_resources();
        }
        Ok(())
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn load_resident_scene_with_exact_test_failure(
        &mut self,
        resident: ResidentSceneCpu,
        failure: renderer::CompleteGpuCandidateTestFailure,
    ) -> Result<(), RendererError> {
        resident.validate_complete()?;
        if self.geometry_path != GeometryPath::PackedAtlas {
            return Err(RendererError::InvalidScene);
        }
        let host = self
            .offscreen_host
            .as_ref()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        let candidate = pollster::block_on(
            renderer::PreparedRuntimeSlot::prepare_complete_gpu_candidate_with_test_failure(
                resident,
                self.exact_offscreen_runtime.as_ref(),
                host.device(),
                host.queue(),
                RENDER_TARGET_FORMAT,
                failure,
            ),
        )
        .map_err(map_prepared_gpu_runtime_error)?;
        self.publish_exact_offscreen_candidate(candidate);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn publish_exact_offscreen_candidate(&mut self, candidate: renderer::PreparedRuntimeSlot) {
        self.exact_offscreen_runtime = Some(candidate);
        self.scene_state.clear_for_exact_runtime();
        self.offscreen_host
            .as_mut()
            .expect("offscreen candidate requires the existing rasterizer")
            .clear_scene_resources();
    }

    /// Prepares the Surface's complete Exact runtime while borrowing
    /// upload-only compact planes from the still-unpublished renderer source.
    /// Failure leaves the source, generations, stats and fallback untouched.
    pub(crate) async fn prepare_surface_exact_candidate(
        &self,
        device: &std::sync::Arc<wgpu::Device>,
        queue: &std::sync::Arc<wgpu::Queue>,
        target_format: wgpu::TextureFormat,
        indirect_execution_supported: bool,
    ) -> Result<renderer::PreparedRuntimeSlot, RendererError> {
        if self.geometry_path != GeometryPath::PackedAtlas {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.offscreen_host.is_some() {
            return Err(RendererError::InvalidConfig);
        }
        let source = self
            .scene_state
            .resident_upload()
            .ok_or(RendererError::SceneNotLoaded)?;
        renderer::PreparedRuntimeSlot::prepare_complete_surface_gpu_candidate(
            source,
            device,
            queue,
            target_format,
            indirect_execution_supported,
        )
        .await
        .map_err(map_prepared_gpu_runtime_error)
    }

    /// Publishes a fully prepared Surface candidate only if the renderer still
    /// owns the exact source allocation used during preparation.
    pub(crate) fn publish_surface_exact_candidate(
        &mut self,
        candidate: renderer::PreparedRuntimeSlot,
    ) -> Result<(), RendererError> {
        if self.geometry_path != GeometryPath::PackedAtlas {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.offscreen_host.is_some() {
            return Err(RendererError::InvalidConfig);
        }
        let source = self
            .scene_state
            .resident_upload()
            .ok_or(RendererError::SceneNotLoaded)?;
        if !candidate.has_same_surface_source(source) {
            return Err(RendererError::SurfacePresenter(
                SurfacePresenterError::SurfaceConfigure(
                    "prepared Surface Exact candidate no longer matches the resident scene".into(),
                ),
            ));
        }
        self.exact_offscreen_runtime = Some(candidate);
        self.scene_state.clear_for_exact_runtime();
        Ok(())
    }

    pub(crate) fn exact_runtime_mut(
        &mut self,
    ) -> Result<&mut renderer::PreparedRuntimeSlot, RendererError> {
        self.exact_offscreen_runtime
            .as_mut()
            .ok_or(RendererError::SceneNotLoaded)
    }

    pub(crate) fn exact_surface_policy(&self) -> Option<renderer::ExactPlanPolicy> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(renderer::PreparedRuntimeSlot::active_policy)
    }

    pub(crate) fn set_exact_surface_policy(
        &mut self,
        policy: renderer::ExactPlanPolicy,
    ) -> Result<(), RendererError> {
        let runtime = self.exact_runtime_mut()?;
        if let renderer::ExactPlanPolicy::Forced(plan) = policy
            && !runtime.plan_is_eligible(plan)
        {
            return Err(SurfacePresenterError::GpuOrderUnsupported.into());
        }
        runtime.set_active_policy(policy);
        Ok(())
    }

    pub(crate) fn exact_surface_plan_is_eligible(&self, plan: plans::PlanId) -> Option<bool> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(|runtime| runtime.plan_is_eligible(plan))
    }

    /// Invalidates only latency-bound whole-plan learning for the active
    /// Surface runtime. Current-stats tickets and prepared resources
    /// deliberately remain in the same semantic generation.
    pub(crate) fn reset_exact_surface_performance_learning(&mut self) {
        if let Some(runtime) = self.exact_offscreen_runtime.as_mut() {
            runtime.reset_surface_performance_learning();
        }
    }

    pub(crate) fn request_exact_surface_cpu_refresh(&mut self) -> Result<(), RendererError> {
        self.exact_runtime_mut()?.request_cpu_order_refresh();
        Ok(())
    }

    pub(crate) fn exact_surface_cpu_refresh_requested(&self) -> Option<bool> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(renderer::PreparedRuntimeSlot::cpu_order_refresh_requested)
    }

    pub(crate) fn exact_surface_last_plan(&self) -> Option<plans::PlanId> {
        self.exact_offscreen_runtime
            .as_ref()
            .and_then(renderer::PreparedRuntimeSlot::last_published_plan)
    }

    pub(crate) fn exact_surface_adaptive_state(
        &self,
    ) -> Option<renderer::ExactAdaptivePolicyState> {
        self.exact_offscreen_runtime
            .as_ref()
            .map(renderer::PreparedRuntimeSlot::adaptive_policy_state)
    }

    pub(crate) fn request_exact_surface_current_stats(
        &mut self,
    ) -> Result<renderer::CurrentStatsRequest, RendererError> {
        Ok(self.exact_runtime_mut()?.request_current_stats())
    }

    pub(crate) fn poll_exact_surface_current_stats(
        &mut self,
    ) -> Result<renderer::CurrentStatsPoll, RendererError> {
        Ok(self.exact_runtime_mut()?.poll_current_stats())
    }

    pub(crate) fn publish_exact_surface_stats(&mut self, stats: FrameStats) {
        debug_assert!(self.exact_offscreen_runtime.is_some());
        self.last_stats = stats;
    }

    /// Commits the CPU side of a successful Surface GPU-resource handoff.
    ///
    /// The presenter must already own complete GPU resources for `uploaded_path`.
    /// Packed staging is validated before it is dropped. Direct/Paged require
    /// the original wide source, while Packed requires an uploadable resident
    /// source; unavailable transitions fail without mutating renderer state.
    pub(crate) fn finish_surface_upload_handoff(
        &mut self,
        uploaded_path: GeometryPath,
    ) -> Result<u64, RendererError> {
        if uploaded_path != self.geometry_path {
            return Err(RendererError::InvalidConfig);
        }
        match uploaded_path {
            GeometryPath::SortedIndexDirect | GeometryPath::PagedActiveAtlas => {
                if self.scene_state.wide().is_none() {
                    return Err(RendererError::GeometrySourceUnavailable {
                        path: uploaded_path,
                    });
                }
                Ok(0)
            }
            GeometryPath::PackedAtlas => {
                let resident = self.scene_state.resident_upload_mut().ok_or(
                    RendererError::GeometrySourceUnavailable {
                        path: GeometryPath::PackedAtlas,
                    },
                )?;
                resident.release_upload_staging().map_err(Into::into)
            }
        }
    }

    pub fn scene(&self) -> Option<&SceneBuffers> {
        self.scene_state.wide()
    }

    /// Returns the compact source retained by the production Packed path.
    pub fn resident_scene(&self) -> Option<&ResidentSceneCpu> {
        self.scene_state.resident_upload().or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.exact_offscreen_runtime
                    .as_ref()
                    .map(|slot| slot.scene().resident())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        })
    }

    /// True for either a wide Direct/Paged source or a compact Packed source.
    pub fn has_scene(&self) -> bool {
        self.scene_state.has_source() || self.resident_scene().is_some()
    }

    pub fn scene_len(&self) -> Option<usize> {
        self.scene_state
            .source_len()
            .or_else(|| self.resident_scene().map(ResidentSceneCpu::len))
    }

    pub fn scene_sh_degree(&self) -> Option<u8> {
        self.scene_state
            .source_sh_degree()
            .or_else(|| self.resident_scene().map(|scene| scene.sh_degree))
    }

    /// Exact source-order world positions shared by CPU ordering, camera
    /// framing, and benchmark traces for every geometry path.
    pub fn positions(&self) -> Option<&[Vec3f]> {
        self.scene_state
            .source_positions()
            .or_else(|| self.resident_scene().map(|scene| scene.positions.as_ref()))
    }

    pub fn world_covariances(&self) -> Option<&[[[f32; 3]; 3]]> {
        self.scene_state
            .direct_inputs()
            .map(|inputs| inputs.world_covariances)
    }

    pub(crate) fn direct_scene_cpu_inputs(
        &self,
    ) -> Option<(&SceneBuffers, &[CameraCovarianceTerms], &[f32])> {
        self.scene_state.direct_inputs().map(|inputs| {
            (
                inputs.scene,
                inputs.world_covariance_terms,
                inputs.alpha_values,
            )
        })
    }

    pub(crate) fn spatial_pages(&self) -> Option<&SpatialPageSet> {
        self.scene_state.spatial_pages()
    }

    #[cfg(test)]
    pub(crate) fn preprocess_capacity(&self) -> usize {
        self.scene_state.preprocess_capacity()
    }

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let positions = self.positions().ok_or(RendererError::SceneNotLoaded)?;
        let mut output = PreprocessOutput {
            depth_keys: Vec::with_capacity(positions.len()),
            indices: Vec::with_capacity(positions.len()),
        };
        preprocess_positions_visible_into(
            positions,
            camera,
            &mut output.depth_keys,
            &mut output.indices,
        )?;
        Ok(output)
    }

    pub fn build_sorted_instances(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<GpuInstance>, FrameStats), RendererError> {
        let mut instances = Vec::new();
        let stats = self.build_sorted_instances_into(camera, &mut instances)?;
        Ok((instances, stats))
    }

    fn preprocess_and_sort_timed(&mut self, camera: &Camera) -> Result<(f32, f32), RendererError> {
        let stable_full32 = self.mode == RenderMode::SortedAlpha;
        let timings = match self.geometry_path {
            GeometryPath::PagedActiveAtlas => {
                let (scene, pages, preprocess_indices) = self
                    .scene_state
                    .paged_order_inputs_mut()
                    .ok_or(RendererError::InvalidScene)?;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let host = self
                        .offscreen_host
                        .as_mut()
                        .ok_or(RendererError::GpuRasterizerUnavailable)?;
                    host.ensure_paged_active_set(scene, pages, camera)?;
                    let entries = host.paged_active_entries()?;
                    self.cpu_order_engine.order_paged(
                        scene,
                        &entries,
                        camera,
                        stable_full32,
                        preprocess_indices,
                    )?
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (scene, pages, preprocess_indices, camera, stable_full32);
                    return Err(RendererError::GpuRasterizerUnavailable);
                }
            }
            GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => {
                let order_engine = &mut self.cpu_order_engine;
                if let Some((positions, preprocess_indices)) =
                    self.scene_state.source_positions_and_preprocess_mut()
                {
                    order_engine.order_positions(
                        CpuPositionView::new(positions),
                        camera,
                        stable_full32,
                        preprocess_indices,
                    )?
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let positions = self
                            .exact_offscreen_runtime
                            .as_ref()
                            .map(|slot| slot.scene().positions())
                            .ok_or(RendererError::SceneNotLoaded)?;
                        order_engine.order_positions(
                            CpuPositionView::new(positions),
                            camera,
                            stable_full32,
                            self.scene_state.preprocess_indices_mut(),
                        )?
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        return Err(RendererError::SceneNotLoaded);
                    }
                }
            }
        };
        Ok((timings.preprocess_ms, timings.sort_ms))
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
            visible_count: self.scene_state.preprocess_indices().len() as u32,
            drawn_count,
        };
        self.last_stats = stats;
        stats
    }

    pub fn build_sorted_instances_into(
        &mut self,
        camera: &Camera,
        instances: &mut Vec<GpuInstance>,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let raster_start = timer_now();
        let inputs = self
            .scene_state
            .direct_inputs()
            .ok_or(RendererError::InvalidScene)?;
        build_instances_into(
            inputs.scene,
            inputs.world_covariances,
            inputs.alpha_values,
            self.scene_state.preprocess_indices(),
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

        let refresh_sort = refresh_sort || self.scene_state.preprocess_indices().is_empty();
        let (preprocess_ms, sort_ms) = if refresh_sort {
            self.preprocess_and_sort_timed(camera)?
        } else {
            camera
                .validate()
                .map_err(|_| RendererError::InvalidCamera)?;
            (0.0, 0.0)
        };

        let drawn_count = self.scene_state.preprocess_indices().len() as u32;
        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, 0.0, drawn_count))
    }

    /// Prepares the Direct order and stats used by one Surface attempt without
    /// replacing the last successfully presented public Renderer snapshot.
    pub(crate) fn prepare_surface_sorted_indices_attempt(
        &mut self,
        camera: &Camera,
        refresh_sort: bool,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();
        let refresh_sort = refresh_sort
            || (self.scene_state.preprocess_indices().is_empty()
                && self.surface_attempt_order.is_none());
        let (preprocess_ms, sort_ms) = if refresh_sort {
            let stable_full32 = self.mode == RenderMode::SortedAlpha;
            let mut candidate = self.surface_attempt_order.take().unwrap_or_default();
            candidate.clear();
            let timings = {
                let order_engine = &mut self.cpu_order_engine;
                let positions = self
                    .scene_state
                    .source_positions()
                    .ok_or(RendererError::SceneNotLoaded)?;
                order_engine.order_positions(
                    CpuPositionView::new(positions),
                    camera,
                    stable_full32,
                    &mut candidate,
                )?
            };
            self.surface_attempt_order = Some(candidate);
            (timings.preprocess_ms, timings.sort_ms)
        } else {
            camera
                .validate()
                .map_err(|_| RendererError::InvalidCamera)?;
            (0.0, 0.0)
        };
        let count =
            u32::try_from(self.surface_sorted_indices_for_attempt().len()).unwrap_or(u32::MAX);
        let stats = FrameStats {
            frame_ms: timer_elapsed_ms(frame_start),
            preprocess_ms,
            sort_ms,
            raster_ms: 0.0,
            visible_count: count,
            drawn_count: count,
        };
        self.surface_attempt_stats = Some(stats);
        Ok(stats)
    }

    /// Installs an async worker result as attempt input while preserving the
    /// public order until a matching primitive present succeeds.
    pub(crate) fn stage_surface_sorted_indices_recycling(
        &mut self,
        indices: &mut Vec<u32>,
    ) -> Result<(), RendererError> {
        let scene_len = self.scene_len().ok_or(RendererError::SceneNotLoaded)?;
        if self.geometry_path != GeometryPath::SortedIndexDirect
            || indices.iter().any(|&index| index as usize >= scene_len)
        {
            return Err(RendererError::InvalidScene);
        }
        let mut candidate = self.surface_attempt_order.take().unwrap_or_default();
        std::mem::swap(&mut candidate, indices);
        self.surface_attempt_order = Some(candidate);
        Ok(())
    }

    pub(crate) fn surface_sorted_indices_for_attempt(&self) -> &[u32] {
        self.surface_attempt_order
            .as_deref()
            .unwrap_or_else(|| self.scene_state.preprocess_indices())
    }

    pub(crate) fn stage_surface_attempt_stats(&mut self, stats: FrameStats) {
        self.surface_attempt_stats = Some(stats);
    }

    /// Infallible commit called only from the successful-present branch.
    pub(crate) fn publish_surface_attempt(&mut self) {
        if let Some(mut order) = self.surface_attempt_order.take() {
            self.scene_state.swap_preprocess_indices(&mut order);
        }
        if let Some(stats) = self.surface_attempt_stats.take() {
            self.last_stats = stats;
        }
    }

    pub(crate) fn publish_surface_stats(&mut self, stats: FrameStats) {
        self.last_stats = stats;
    }

    fn discard_surface_attempt(&mut self) {
        self.surface_attempt_order = None;
        self.surface_attempt_stats = None;
    }

    pub fn current_sorted_indices(&self) -> &[u32] {
        #[cfg(not(target_arch = "wasm32"))]
        if self.geometry_path == GeometryPath::PackedAtlas
            && let Some(order) = self
                .exact_offscreen_runtime
                .as_ref()
                .and_then(renderer::PreparedRuntimeSlot::last_usable_cpu_order)
        {
            return order;
        }
        self.scene_state.preprocess_indices()
    }

    pub fn replace_surface_sorted_indices(
        &mut self,
        mut indices: Vec<u32>,
    ) -> Result<(), RendererError> {
        self.replace_surface_sorted_indices_recycling(&mut indices)
    }

    pub(crate) fn replace_surface_sorted_indices_recycling(
        &mut self,
        indices: &mut Vec<u32>,
    ) -> Result<(), RendererError> {
        let scene_len = self.scene_len().ok_or(RendererError::SceneNotLoaded)?;
        match self.geometry_path {
            GeometryPath::PagedActiveAtlas => {
                let max_index = self
                    .scene_state
                    .spatial_pages()
                    .map(|pages| {
                        pages
                            .page_count()
                            .saturating_mul(pages.page_capacity)
                            .saturating_sub(1)
                    })
                    .unwrap_or(0) as u32;
                if indices.iter().any(|&idx| idx > max_index) {
                    return Err(RendererError::InvalidScene);
                }
            }
            GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => {
                if indices.iter().any(|&idx| idx as usize >= scene_len) {
                    return Err(RendererError::InvalidScene);
                }
            }
        }

        self.scene_state.swap_preprocess_indices(indices);
        Ok(())
    }

    pub fn build_sorted_indices(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<u32>, FrameStats), RendererError> {
        let frame_start = timer_now();

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let drawn_count = self.scene_state.preprocess_indices().len() as u32;
        let stats = self.record_stats(frame_start, preprocess_ms, sort_ms, 0.0, drawn_count);
        Ok((self.scene_state.preprocess_indices().to_vec(), stats))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn raster_sorted_indices(
        &mut self,
        camera: &Camera,
        sorted_indices: &[u32],
    ) -> Result<(), RendererError> {
        match self.geometry_path {
            GeometryPath::SortedIndexDirect => {
                let inputs = self
                    .scene_state
                    .direct_inputs()
                    .ok_or(RendererError::InvalidScene)?;
                self.offscreen_host
                    .as_mut()
                    .ok_or(RendererError::GpuRasterizerUnavailable)?
                    .render_direct_sorted_indices(
                        self.config,
                        sorted_indices,
                        camera,
                        inputs.scene,
                        inputs.world_covariance_terms,
                        inputs.alpha_values,
                    )
            }
            GeometryPath::PackedAtlas => self
                .offscreen_host
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?
                .render_packed_sorted_indices(
                    self.config,
                    sorted_indices,
                    camera,
                    self.scene_state
                        .resident_upload()
                        .ok_or(RendererError::SceneNotLoaded)?,
                ),
            GeometryPath::PagedActiveAtlas => self
                .offscreen_host
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?
                .render_paged_sorted_indices(
                    self.config,
                    sorted_indices,
                    camera,
                    self.scene_state
                        .wide()
                        .ok_or(RendererError::SceneNotLoaded)?,
                ),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        let frame_start = timer_now();
        if self.offscreen_host.is_none() {
            return Err(RendererError::GpuRasterizerUnavailable);
        }

        if self.geometry_path == GeometryPath::PackedAtlas && self.exact_offscreen_runtime.is_some()
        {
            return self.render_packed_exact_offscreen(camera, frame_start);
        }

        let (preprocess_ms, sort_ms) = self.preprocess_and_sort_timed(camera)?;

        let raster_start = timer_now();
        let sorted_indices = std::mem::take(self.scene_state.preprocess_indices_mut());
        let raster_result = self.raster_sorted_indices(camera, &sorted_indices);
        let drawn_count = sorted_indices.len() as u32;
        *self.scene_state.preprocess_indices_mut() = sorted_indices;
        raster_result?;
        let raster_ms = timer_elapsed_ms(raster_start);

        Ok(self.record_stats(frame_start, preprocess_ms, sort_ms, raster_ms, drawn_count))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_packed_exact_offscreen(
        &mut self,
        camera: &Camera,
        frame_start: TimerInstant,
    ) -> Result<FrameStats, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        let viewport = renderer::frame::Viewport::new(self.config.width, self.config.height)
            .map_err(|_| RendererError::InvalidConfig)?;
        let host = self
            .offscreen_host
            .as_ref()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        let slot = self
            .exact_offscreen_runtime
            .as_mut()
            .ok_or(RendererError::SceneNotLoaded)?;
        let request = renderer::GpuFrameEncodeRequest::new(
            plans::PlanId::CpuPostSort,
            camera,
            viewport,
            host.target_view(),
            RENDER_TARGET_FORMAT,
            wgpu::Color::TRANSPARENT,
        )
        .with_forced_cpu_order_refresh()
        .with_host_frame_started(frame_start);
        let pending =
            renderer::encode_frame_gpu(slot, request).map_err(map_frame_execution_error)?;
        let submission =
            renderer::submit_encoded_frame(slot, pending).map_err(map_frame_execution_error)?;
        let timings = submission
            .host_timings()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let visible_count = submission
            .visible_count()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let drawn_count = submission
            .draw_count()
            .ok_or(RendererError::GpuDeviceCreation)?;
        let stats = FrameStats {
            frame_ms: timings.frame_ms(),
            preprocess_ms: timings.preprocess_ms(),
            sort_ms: timings.sort_ms(),
            raster_ms: timings.raster_ms(),
            visible_count,
            drawn_count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn render_frame_with_external_order_for_test(
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
            let host = self
                .offscreen_host
                .as_mut()
                .ok_or(RendererError::GpuRasterizerUnavailable)?;
            host.readback_rgba8()
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
}
