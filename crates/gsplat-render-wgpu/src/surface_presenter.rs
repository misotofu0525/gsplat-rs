//! WGPU Surface presentation and geometry-resource ownership.

use gsplat_core::{Camera, SceneBuffers};
use gsplat_sort::CpuSortBackend;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use crate::SurfaceRasterExecutionPlan;
use crate::gpu_producer_telemetry::SurfaceGpuOrderProducer;
use crate::gpu_telemetry::{
    CpuOrderCompletionTelemetry, CpuOrderTelemetryPoll, FrameInstanceCounts, GpuOrderTelemetryPoll,
    TelemetrySubmission,
};
use crate::packed_gpu;
use crate::paged_active_set::PagedActiveSet;
use crate::raster::{QUAD_VERTEX_COUNT, SplatDraw, encode_splat_draw_into};
use crate::resident_gpu;
#[cfg(not(target_arch = "wasm32"))]
use crate::surface::SurfaceCapture;
pub use crate::surface::SurfaceFrameCapture;
use crate::surface::shadow::{
    NativeSurfaceExactHost, SurfaceExactFrameResult, SurfaceExactRequest,
    render_surface_exact_frame,
};
use crate::surface::standalone_direct_runtime::{
    DirectGpuTelemetrySample, PreparedStandaloneDirectScene, StandaloneDirectRuntime,
};
use crate::surface::{
    SurfaceConfigurationOwner, SurfaceLifecycle, create_surface_instance, select_present_mode,
};
use crate::{
    DEFAULT_PAGED_ATLAS_SLOTS, DirectSceneError, DirectScenePath, DirectScenePreflight,
    GeometryPath, PackedScenePath, PackedScenePreflight, Renderer, ResidentGpuBytePlan,
    SpatialPageSet, SurfacePresenterError, TimerInstant, direct_scene_preflight,
    packed_scene_preflight_with_limits, preprocess_paged_visible_into, refresh_paged_hot_colors,
    wgpu_label,
};
pub(crate) struct SurfacePagedRuntime {
    pub(crate) active_set: PagedActiveSet,
    sort_backend: CpuSortBackend,
    depth_keys: Vec<u32>,
    sorted_indices: Vec<u32>,
}

impl SurfacePagedRuntime {
    pub(crate) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        pages: SpatialPageSet,
    ) -> Result<Self, SurfacePresenterError> {
        let active_set = PagedActiveSet::new(device, layout, scene, pages)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        Ok(Self {
            active_set,
            sort_backend: CpuSortBackend::default(),
            depth_keys: Vec::new(),
            sorted_indices: Vec::new(),
        })
    }

    pub(crate) fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        scene: &SceneBuffers,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<u32, SurfacePresenterError> {
        self.active_set
            .sync(queue, scene, camera)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        let entries = self.active_set.atlas.active_entries();
        preprocess_paged_visible_into(
            scene,
            &entries,
            camera,
            &mut self.depth_keys,
            &mut self.sorted_indices,
        )
        .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        self.sort_backend
            .sort_values_by_keys(&self.depth_keys, &mut self.sorted_indices)
            .map_err(|err| SurfacePresenterError::PagedAtlas(err.to_string()))?;
        refresh_paged_hot_colors(queue, &mut self.active_set.atlas, scene, camera);
        self.active_set
            .atlas
            .resources
            .prepare(queue, &self.sorted_indices, camera, width, height, true)
            .map_err(SurfacePresenterError::from)
    }
}

/// Surface/device presentation leaves shared with the renderer-owned Exact
/// runtime. This host deliberately owns no legacy geometry, pipeline, order,
/// projection, or telemetry graph.
pub(crate) struct SurfacePresenterHost {
    surface: wgpu::Surface<'static>,
    adapter_info: wgpu::AdapterInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_configuration: SurfaceConfigurationOwner,
    surface_lifecycle: SurfaceLifecycle,
    #[cfg(not(target_arch = "wasm32"))]
    surface_capture: SurfaceCapture,
    adapter_max_storage_buffers_per_shader_stage: u32,
    adapter_max_storage_buffer_binding_size: u64,
    indirect_execution_supported: bool,
    timestamp_queries_enabled: bool,
    addressable_splat_count: usize,
}

pub struct SurfacePresenter {
    host: SurfacePresenterHost,
    direct_runtime: StandaloneDirectRuntime,
    paged_pipeline: wgpu::RenderPipeline,
    paged_bind_group_layout: wgpu::BindGroupLayout,
    paged_instance_count: u32,
    geometry: SurfaceGeometry,
    cpu_order_completion_telemetry: CpuOrderCompletionTelemetry,
}

#[derive(Clone, Copy)]
pub(crate) struct CpuCompletionSampleRequest {
    pub(crate) camera_revision: u64,
    pub(crate) started: TimerInstant,
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
}

enum SurfaceGeometry {
    Direct,
    Paged(Box<SurfacePagedRuntime>),
}

enum PreparedSurfaceGeometry {
    Direct(PreparedStandaloneDirectScene),
    Paged(Box<SurfacePagedRuntime>),
}

impl PreparedSurfaceGeometry {
    fn addressable_splat_count(&self) -> usize {
        match self {
            Self::Direct(direct) => direct.addressable_splat_count(),
            Self::Paged(paged) => paged.active_set.atlas.resources.capacity,
        }
    }
}

impl SurfaceGeometry {
    const fn path(&self) -> GeometryPath {
        match self {
            Self::Direct => GeometryPath::SortedIndexDirect,
            Self::Paged(_) => GeometryPath::PagedActiveAtlas,
        }
    }
}

fn supports_direct_gpu_order(downlevel: &wgpu::DownlevelCapabilities) -> bool {
    downlevel
        .flags
        .contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION)
}

/// Device-local dependencies shared by every geometry-path constructor.
/// Keeping them together makes initial creation and transactional path
/// switching use the same resource factory contract.
struct GeometryResourceContext<'a> {
    device: &'a wgpu::Device,
    direct_runtime: &'a StandaloneDirectRuntime,
    paged_bind_group_layout: &'a wgpu::BindGroupLayout,
}

fn create_geometry_resources(
    context: GeometryResourceContext<'_>,
    path: GeometryPath,
    renderer: &Renderer,
) -> Result<PreparedSurfaceGeometry, SurfacePresenterError> {
    let GeometryResourceContext {
        device,
        direct_runtime,
        paged_bind_group_layout,
    } = context;
    match path {
        GeometryPath::SortedIndexDirect => {
            let scene = renderer.scene().ok_or_else(|| {
                if renderer.has_scene() {
                    SurfacePresenterError::GeometrySourceUnavailable { path }
                } else {
                    SurfacePresenterError::SceneNotLoaded
                }
            })?;
            let world_covariance_terms = renderer
                .world_covariance_terms
                .as_deref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let alpha_values = renderer
                .alpha_values
                .as_deref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let direct_scene = direct_runtime.prepare_scene_candidate(
                device,
                scene,
                world_covariance_terms,
                alpha_values,
            )?;
            Ok(PreparedSurfaceGeometry::Direct(direct_scene))
        }
        GeometryPath::PackedAtlas => {
            Err(SurfacePresenterError::StandalonePackedPresenterUnsupported)
        }
        GeometryPath::PagedActiveAtlas => {
            let scene = renderer.scene().ok_or_else(|| {
                if renderer.has_scene() {
                    SurfacePresenterError::GeometrySourceUnavailable { path }
                } else {
                    SurfacePresenterError::SceneNotLoaded
                }
            })?;
            let pages = renderer
                .spatial_pages
                .clone()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let paged_scene =
                SurfacePagedRuntime::new(device, paged_bind_group_layout, scene, pages)?;
            Ok(PreparedSurfaceGeometry::Paged(Box::new(paged_scene)))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceResourcePlan {
    pub(crate) geometry_path: GeometryPath,
    pub(crate) direct_preflight: DirectScenePreflight,
    pub(crate) packed_preflight: PackedScenePreflight,
    pub(crate) resident_plan: ResidentGpuBytePlan,
    pub(crate) paged_plan: packed_gpu::PagedCompactBytePlan,
    pub(crate) required_texture_dimension: u32,
}

impl SurfaceResourcePlan {
    pub(crate) fn validate_selected_path(
        self,
        limits: &wgpu::Limits,
    ) -> Result<(), SurfacePresenterError> {
        match self.geometry_path {
            GeometryPath::SortedIndexDirect
                if self.direct_preflight.path == DirectScenePath::ActiveAtlasRequired =>
            {
                Err(DirectSceneError::ResourceLimitExceeded(Box::new(self.direct_preflight)).into())
            }
            GeometryPath::PackedAtlas => {
                if self.packed_preflight.path == PackedScenePath::PagingRequired {
                    return Err(DirectSceneError::PackedResourceLimitExceeded(Box::new(
                        self.packed_preflight,
                    ))
                    .into());
                }
                self.resident_plan.validate_limits(limits)?;
                Ok(())
            }
            GeometryPath::PagedActiveAtlas => {
                self.paged_plan.validate()?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_resource_plan(
    geometry_path: GeometryPath,
    scene_splats: usize,
    sh_degree: u8,
    page_count: usize,
    page_capacity: usize,
    width: u32,
    height: u32,
    limits: &wgpu::Limits,
) -> Result<SurfaceResourcePlan, DirectSceneError> {
    let direct_preflight = direct_scene_preflight(scene_splats, sh_degree, limits)?;
    let paged_resident_capacity = match geometry_path {
        GeometryPath::PagedActiveAtlas => {
            let slot_count = page_count.clamp(1, DEFAULT_PAGED_ATLAS_SLOTS);
            slot_count
                .checked_mul(page_capacity.max(1))
                .ok_or(DirectSceneError::ResourceSizeOverflow)?
        }
        GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => scene_splats,
    };
    let packed_preflight = packed_scene_preflight_with_limits(scene_splats, sh_degree, limits)?;
    let resident_plan = packed_preflight.resident_gpu;
    let paged_plan = packed_gpu::PagedCompactBytePlan::for_count(
        paged_resident_capacity,
        u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size),
    )?;
    let required_texture_dimension = width.max(height);

    Ok(SurfaceResourcePlan {
        geometry_path,
        direct_preflight,
        packed_preflight,
        resident_plan,
        paged_plan,
        required_texture_dimension,
    })
}

pub(crate) fn try_prepare_then_commit<State, Prepared, Error>(
    state: &mut State,
    prepare: impl FnOnce(&State) -> Result<Prepared, Error>,
    commit: impl FnOnce(&mut State, Prepared),
) -> Result<(), Error> {
    let prepared = prepare(state)?;
    commit(state, prepared);
    Ok(())
}

fn surface_required_device_limits(
    adapter_limits: &wgpu::Limits,
    resource_plan: &SurfaceResourcePlan,
) -> Result<wgpu::Limits, SurfacePresenterError> {
    resource_plan.validate_selected_path(adapter_limits)?;
    if resource_plan.required_texture_dimension > adapter_limits.max_texture_dimension_2d {
        return Err(SurfacePresenterError::DeviceCreation(format!(
            "required texture dimension {} exceeds adapter limit {}",
            resource_plan.required_texture_dimension, adapter_limits.max_texture_dimension_2d
        )));
    }

    let required_storage_bytes = match resource_plan.geometry_path {
        GeometryPath::SortedIndexDirect => resource_plan
            .direct_preflight
            .requirements
            .iter()
            .map(|requirement| requirement.required_bytes)
            .max()
            .unwrap_or(0),
        GeometryPath::PackedAtlas => resource_plan.resident_plan.largest_storage_binding_bytes(),
        GeometryPath::PagedActiveAtlas => resource_plan
            .paged_plan
            .sorted_indices_bytes
            .max(resource_plan.paged_plan.hot_record_storage_bytes),
    };
    let required_storage_binding_size = u32::try_from(required_storage_bytes).map_err(|_| {
        SurfacePresenterError::DeviceCreation(format!(
            "selected geometry path requires a {required_storage_bytes}-byte storage binding, exceeding the wgpu limit representation"
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
    if resource_plan.geometry_path == GeometryPath::PackedAtlas {
        required_limits.max_storage_buffers_per_shader_stage = required_limits
            .max_storage_buffers_per_shader_stage
            .max(resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS);
    }

    if !required_limits.check_limits(adapter_limits) {
        return Err(SurfacePresenterError::DeviceCreation(format!(
            "surface requirements exceed adapter capabilities; requested={required_limits:?}; adapter={adapter_limits:?}"
        )));
    }
    Ok(required_limits)
}

impl SurfacePresenterHost {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn from_window<T>(
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

    /// # Safety
    ///
    /// The caller must keep both raw handles valid until this host is dropped.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) unsafe fn from_raw_handles(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        pollster::block_on(unsafe {
            Self::from_raw_handles_async(
                raw_display_handle,
                raw_window_handle,
                width,
                height,
                renderer,
            )
        })
    }

    async unsafe fn from_raw_handles_async(
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
    pub(crate) async fn from_canvas(
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
        let adapter_info = adapter.get_info();
        let adapter_features = adapter.features();
        let downlevel = adapter.get_downlevel_capabilities();
        let indirect_execution_supported = supports_direct_gpu_order(&downlevel);
        let timestamp_queries_enabled = adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY)
            && downlevel
                .flags
                .contains(wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE);
        let scene_splats = renderer
            .scene_len()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let sh_degree = renderer
            .scene_sh_degree()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let geometry_path = renderer.geometry_path();
        let (page_count, page_capacity) = renderer
            .spatial_pages
            .as_ref()
            .map(|pages| (pages.page_count(), pages.page_capacity))
            .unwrap_or_default();
        if geometry_path == GeometryPath::PagedActiveAtlas && page_count == 0 {
            return Err(SurfacePresenterError::SceneNotLoaded);
        }
        let resource_plan = surface_resource_plan(
            geometry_path,
            scene_splats,
            sh_degree,
            page_count,
            page_capacity,
            width,
            height,
            &adapter_limits,
        )?;
        let required_limits = surface_required_device_limits(&adapter_limits, &resource_plan)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-surface-device"),
                required_features: if timestamp_queries_enabled {
                    wgpu::Features::TIMESTAMP_QUERY
                        | if cfg!(any(target_os = "macos", target_os = "ios")) {
                            wgpu::Features::empty()
                        } else {
                            adapter_features & wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
                        }
                } else {
                    wgpu::Features::empty()
                },
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
        let surface_configuration = SurfaceConfigurationOwner::new_configured(
            &surface,
            &device,
            format,
            width,
            height,
            present_mode,
            alpha_mode,
            device.limits().max_texture_dimension_2d.max(1),
        )
        .await?;

        Ok(Self {
            surface,
            adapter_info,
            device,
            queue,
            surface_configuration,
            surface_lifecycle: SurfaceLifecycle::new(),
            #[cfg(not(target_arch = "wasm32"))]
            surface_capture: SurfaceCapture::new(
                caps.usages.contains(wgpu::TextureUsages::COPY_SRC),
            ),
            adapter_max_storage_buffers_per_shader_stage: adapter_limits
                .max_storage_buffers_per_shader_stage,
            adapter_max_storage_buffer_binding_size: u64::from(
                adapter_limits.max_storage_buffer_binding_size,
            )
            .min(adapter_limits.max_buffer_size),
            indirect_execution_supported,
            timestamp_queries_enabled,
            addressable_splat_count: scene_splats,
        })
    }

    pub(crate) const fn surface_size(&self) -> (u32, u32) {
        self.surface_configuration.size()
    }

    pub(crate) const fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    pub(crate) const fn addressable_splat_count(&self) -> usize {
        self.addressable_splat_count
    }

    pub(crate) const fn adapter_max_storage_buffers_per_shader_stage(&self) -> u32 {
        self.adapter_max_storage_buffers_per_shader_stage
    }

    pub(crate) const fn adapter_max_storage_buffer_binding_size(&self) -> u64 {
        self.adapter_max_storage_buffer_binding_size
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfacePresenterError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.prepare_native_resize(width, height)? {
                self.commit_native_resize(width, height);
            }
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.surface_configuration.validate_size(width, height)?;
            if self.surface_configuration.resize_required(width, height) {
                Err(SurfacePresenterError::SurfaceResizePreparationRequired)
            } else {
                Ok(())
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_native_resize(
        &self,
        width: u32,
        height: u32,
    ) -> Result<bool, SurfacePresenterError> {
        ensure_surface_capture_allows_resize(self.surface_capture.has_pending())?;
        self.surface_configuration.validate_size(width, height)?;
        Ok(self.surface_configuration.resize_required(width, height))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn commit_native_resize(&mut self, width: u32, height: u32) {
        self.surface_configuration
            .resize_native(&self.surface, &self.device, width, height);
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn resize_async(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        self.surface_configuration.validate_size(width, height)?;
        if !self.surface_configuration.resize_required(width, height) {
            return Ok(());
        }
        self.surface_configuration
            .resize_transactionally(&self.surface, &self.device, width, height)
            .await?;
        self.surface_lifecycle.begin_frame();
        Ok(())
    }

    pub(crate) fn set_frame_latency(&mut self, latency: u32) -> bool {
        self.surface_configuration
            .update_frame_latency(&self.surface, &self.device, latency)
    }

    pub(crate) const fn last_presented_size(&self) -> Option<(u32, u32)> {
        self.surface_lifecycle.last_presented_size()
    }

    pub(crate) const fn last_frame_presented(&self) -> bool {
        self.surface_lifecycle.last_frame_presented()
    }

    pub(crate) const fn gpu_order_timestamps_enabled(&self) -> bool {
        self.timestamp_queries_enabled
    }

    /// Advances callbacks for host-owned queue work without consulting any
    /// legacy presenter telemetry graph.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pump_receipt_callbacks(
        &self,
        timeout: Duration,
    ) -> Result<bool, crate::RendererError> {
        crate::pump_device_receipt_callbacks(&self.device, timeout)
    }

    pub(crate) fn exact_runtime_context(
        &self,
    ) -> (
        std::sync::Arc<wgpu::Device>,
        std::sync::Arc<wgpu::Queue>,
        wgpu::TextureFormat,
        bool,
    ) {
        (
            std::sync::Arc::new(self.device.clone()),
            std::sync::Arc::new(self.queue.clone()),
            self.surface_configuration.format(),
            self.indirect_execution_supported,
        )
    }

    pub(crate) fn render_exact_frame(
        &mut self,
        runtime: &mut crate::renderer::PreparedRuntimeSlot,
        camera: &Camera,
        force_cpu_order_refresh: bool,
        host_frame_started: TimerInstant,
    ) -> Result<Option<SurfaceExactFrameResult>, crate::surface::shadow::SurfaceExactError> {
        let (width, height) = self.surface_configuration.size();
        let viewport = crate::renderer::frame::Viewport::new(width, height)
            .expect("validated Surface configuration has a non-zero viewport");
        render_surface_exact_frame(
            runtime,
            NativeSurfaceExactHost {
                surface: &self.surface,
                device: &self.device,
                configuration: &self.surface_configuration,
                lifecycle: &mut self.surface_lifecycle,
                #[cfg(not(target_arch = "wasm32"))]
                capture: &mut self.surface_capture,
            },
            SurfaceExactRequest {
                camera,
                viewport,
                clear: wgpu::Color::BLACK,
                force_cpu_order_refresh,
                host_frame_started: Some(host_frame_started),
            },
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request_surface_capture(&mut self) -> Result<(), SurfacePresenterError> {
        self.surface_configuration.ensure_capture_valid()?;
        let (width, height) = self.surface_configuration.size();
        let format = self.surface_configuration.format();
        let pending = self
            .surface_capture
            .prepare_request(&self.device, width, height, format)?;
        pollster::block_on(
            self.surface_configuration
                .ensure_copy_src(&self.surface, &self.device),
        )?;
        self.surface_capture.publish(pending);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cancel_surface_capture(&mut self) -> bool {
        self.surface_capture.cancel()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_surface_capture(
        &mut self,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        self.surface_capture.take(&self.device)
    }
}

impl SurfacePresenter {
    fn admit_standalone_geometry(path: GeometryPath) -> Result<(), SurfacePresenterError> {
        match path {
            GeometryPath::PackedAtlas => {
                Err(SurfacePresenterError::StandalonePackedPresenterUnsupported)
            }
            GeometryPath::SortedIndexDirect | GeometryPath::PagedActiveAtlas => Ok(()),
        }
    }

    /// Creates a standalone presenter for an owned native window target.
    ///
    /// Direct and diagnostic Paged geometry are supported. Packed must be
    /// constructed through [`crate::SurfaceRenderSession::from_window`] so the
    /// session allocates only its Exact host graph.
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
        Self::admit_standalone_geometry(renderer.geometry_path())?;
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
        let host = SurfacePresenterHost::from_window(target, width, height, renderer).await?;
        Self::from_host_async(host, renderer).await
    }

    /// Creates a standalone presenter from raw embedding-platform handles.
    ///
    /// Direct and diagnostic Paged geometry are supported. Packed must be
    /// constructed through [`crate::SurfaceRenderSession::from_raw_handles`]
    /// so the session allocates only its Exact host graph.
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
        Self::admit_standalone_geometry(renderer.geometry_path())?;
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
        let host = unsafe {
            SurfacePresenterHost::from_raw_handles_async(
                raw_display_handle,
                raw_window_handle,
                width,
                height,
                renderer,
            )
            .await?
        };
        Self::from_host_async(host, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    /// Creates a standalone presenter for a browser canvas.
    ///
    /// Direct and diagnostic Paged geometry are supported. Packed must be
    /// constructed through [`crate::SurfaceRenderSession::from_canvas`] so the
    /// session allocates only its Exact host graph.
    pub async fn from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        Self::admit_standalone_geometry(renderer.geometry_path())?;
        Self::from_canvas_selected(canvas, width, height, renderer).await
    }

    #[cfg(target_arch = "wasm32")]
    async fn from_canvas_selected(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        let host = SurfacePresenterHost::from_canvas(canvas, width, height, renderer).await?;
        Self::from_host_async(host, renderer).await
    }

    async fn from_host_async(
        mut host: SurfacePresenterHost,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        let geometry_path = renderer.geometry_path();
        let format = host.surface_configuration.format();
        let timestamp_queries_enabled = host.timestamp_queries_enabled;
        let device = &host.device;
        let queue = &host.queue;

        // Every shared pipeline/layout and the selected geometry is one
        // unpublished candidate. WebGPU reports constructor failures only
        // when these async scopes are popped, so do not build or expose any
        // part of the presenter outside this transaction.
        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let mut direct_runtime =
            StandaloneDirectRuntime::new(device, queue, format, timestamp_queries_enabled);
        let paged_bind_group_layout = packed_gpu::create_packed_bind_group_layout(device);
        let paged_pipeline =
            packed_gpu::create_packed_pipeline(device, &paged_bind_group_layout, format);
        let geometry_result = create_geometry_resources(
            GeometryResourceContext {
                device,
                direct_runtime: &direct_runtime,
                paged_bind_group_layout: &paged_bind_group_layout,
            },
            geometry_path,
            renderer,
        );
        let cpu_order_completion_telemetry = CpuOrderCompletionTelemetry::default();
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if oom_error.is_some() {
            return Err(SurfacePresenterError::SurfaceOutOfMemory);
        }
        if let Some(error) = internal_error.or(validation_error) {
            return Err(SurfacePresenterError::DeviceCreation(format!(
                "surface geometry resource creation failed: {error}"
            )));
        }
        let prepared_geometry = geometry_result?;
        host.addressable_splat_count = prepared_geometry.addressable_splat_count();
        let geometry = match prepared_geometry {
            PreparedSurfaceGeometry::Direct(scene) => {
                direct_runtime.publish_scene(scene);
                SurfaceGeometry::Direct
            }
            PreparedSurfaceGeometry::Paged(paged) => SurfaceGeometry::Paged(paged),
        };

        Ok(Self {
            host,
            direct_runtime,
            paged_pipeline,
            paged_bind_group_layout,
            paged_instance_count: 0,
            geometry,
            cpu_order_completion_telemetry,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfacePresenterError> {
        #[cfg(not(target_arch = "wasm32"))]
        let resize_required = self.host.prepare_native_resize(width, height)?;
        #[cfg(target_arch = "wasm32")]
        self.host
            .surface_configuration
            .validate_size(width, height)?;
        #[cfg(target_arch = "wasm32")]
        let resize_required = self
            .host
            .surface_configuration
            .resize_required(width, height);
        if !resize_required {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err(SurfacePresenterError::SurfaceResizePreparationRequired)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.host.commit_native_resize(width, height);
            self.direct_runtime.invalidate_gpu_order_telemetry();
            self.cpu_order_completion_telemetry.invalidate_generation();
            Ok(())
        }
    }

    /// The standalone browser presenter retains its historical fail-closed
    /// resize boundary. Product Packed resizing is owned by
    /// [`crate::SurfaceRenderSession::from_canvas`] and its host transaction.
    #[cfg(target_arch = "wasm32")]
    pub async fn resize_async(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        self.host
            .surface_configuration
            .validate_size(width, height)?;
        Err(SurfacePresenterError::SurfaceResizeUnsupported)
    }

    pub const fn surface_size(&self) -> (u32, u32) {
        self.host.surface_configuration.size()
    }

    /// Arms a one-shot exact framebuffer readback for the next presented
    /// native frame. The first request upgrades this Surface to `COPY_SRC`;
    /// normal product sessions remain render-attachment-only forever unless a
    /// caller explicitly opts into this diagnostic path.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request_surface_capture(&mut self) -> Result<(), SurfacePresenterError> {
        self.host.request_surface_capture()
    }

    /// Cancels an armed capture that has not yet been taken.
    ///
    /// The Surface may remain configured with `COPY_SRC`; that diagnostic
    /// capability is harmless after the readback buffer is released and
    /// avoids a second fallible swapchain transition during recovery.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cancel_surface_capture(&mut self) -> bool {
        self.host.cancel_surface_capture()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_surface_capture(
        &mut self,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        self.host.take_surface_capture()
    }

    /// Actual dimensions of the raster target before presentation.
    pub fn internal_render_size(&self) -> (u32, u32) {
        self.surface_size()
    }

    /// Physical adapter identity selected for this Surface presenter.
    ///
    /// This is an observation-only receipt. It cannot select a backend,
    /// change device capabilities, or influence renderer plan policy.
    pub const fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.host.adapter_info
    }

    /// Number of splat records allocated by the selected Surface geometry.
    pub const fn addressable_splat_count(&self) -> usize {
        self.host.addressable_splat_count
    }

    /// Physical adapter storage-binding count used by exactness admission.
    pub const fn adapter_max_storage_buffers_per_shader_stage(&self) -> u32 {
        self.host.adapter_max_storage_buffers_per_shader_stage
    }

    /// Effective physical adapter size for one storage-buffer binding.
    pub const fn adapter_max_storage_buffer_binding_size(&self) -> u64 {
        self.host.adapter_max_storage_buffer_binding_size
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        if !self.host.set_frame_latency(latency) {
            return;
        }
        self.direct_runtime.invalidate_gpu_order_telemetry();
        self.cpu_order_completion_telemetry.invalidate_generation();
    }

    pub const fn geometry_path(&self) -> GeometryPath {
        self.geometry.path()
    }

    /// Whether the most recent top-level render call actually presented a
    /// drawable. Exact-count preparation, timeouts, and errors leave this
    /// false.
    pub(crate) const fn last_frame_presented(&self) -> bool {
        self.host.surface_lifecycle.last_frame_presented()
    }

    pub(crate) const fn last_presented_size(&self) -> Option<(u32, u32)> {
        self.host.surface_lifecycle.last_presented_size()
    }

    /// Direct and diagnostic Paged standalone presenters use global quads.
    /// Product Packed raster identity is reported by `SurfaceRenderSession`.
    pub const fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        SurfaceRasterExecutionPlan::GlobalQuads
    }

    /// Standalone Direct supports only the PostSort producer identity.
    /// Product Packed producer preparation is renderer-owned.
    pub async fn prepare_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), SurfacePresenterError> {
        match producer {
            SurfaceGpuOrderProducer::PostSort => self.prepare_post_sort_gpu_order().await,
            SurfaceGpuOrderProducer::Preproject => {
                Err(SurfacePresenterError::PreprojectProducerIncompatible)
            }
        }
    }

    /// Force an A/B raster plan without changing CPU/GPU ordering policy.
    /// There is deliberately no point-count heuristic here; strategy code can
    /// evaluate measured telemetry and call this explicit knob later.
    pub fn set_raster_execution_plan(
        &mut self,
        plan: SurfaceRasterExecutionPlan,
    ) -> Result<(), SurfacePresenterError> {
        if plan == SurfaceRasterExecutionPlan::GlobalQuads {
            Ok(())
        } else {
            Err(SurfacePresenterError::GpuOrderUnsupported)
        }
    }

    /// Switches the Surface geometry path when the requested transition is
    /// supported.
    ///
    /// Requesting the active path is idempotent and does not prepare resources.
    /// Web geometry is constructor-only, so every changed browser request is
    /// rejected as Unsupported before preparation. On native, a changed
    /// transition entering or leaving [`GeometryPath::PackedAtlas`] is
    /// rejected before resource preparation; Direct/Paged transitions remain
    /// transactional and leave the current path intact if preparation fails.
    /// Callers must keep `renderer`'s loaded scene in sync with the presenter
    /// that was created from it.
    pub fn set_geometry_path(
        &mut self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<(), SurfacePresenterError> {
        let current = self.geometry.path();
        if current == path {
            return Ok(());
        }
        if cfg!(target_arch = "wasm32") {
            return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported);
        }
        if path == GeometryPath::PackedAtlas {
            return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported);
        }

        try_prepare_then_commit(
            self,
            |presenter| presenter.prepare_geometry_resources(path, renderer),
            |presenter, prepared| {
                presenter.host.addressable_splat_count = prepared.addressable_splat_count();
                presenter.geometry = match prepared {
                    PreparedSurfaceGeometry::Direct(scene) => {
                        presenter.direct_runtime.publish_scene(scene);
                        SurfaceGeometry::Direct
                    }
                    PreparedSurfaceGeometry::Paged(paged) => {
                        presenter.direct_runtime.clear_scene();
                        SurfaceGeometry::Paged(paged)
                    }
                };
                presenter.paged_instance_count = 0;
                presenter.direct_runtime.invalidate_gpu_order_telemetry();
                presenter
                    .cpu_order_completion_telemetry
                    .invalidate_generation();
            },
        )
    }

    fn prepare_geometry_resources(
        &self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<PreparedSurfaceGeometry, SurfacePresenterError> {
        create_geometry_resources(
            GeometryResourceContext {
                device: &self.host.device,
                direct_runtime: &self.direct_runtime,
                paged_bind_group_layout: &self.paged_bind_group_layout,
            },
            path,
            renderer,
        )
    }

    pub fn render_sorted_indices(
        &mut self,
        scene: &SceneBuffers,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.host.surface_lifecycle.begin_frame();
        if !matches!(self.geometry, SurfaceGeometry::Paged(_)) {
            return self.render_cpu_sorted_indices(sorted_indices, camera, refresh_indices);
        }
        self.paged_instance_count = match &mut self.geometry {
            SurfaceGeometry::Paged(paged) => paged.prepare(
                &self.host.queue,
                scene,
                camera,
                self.host.surface_configuration.size().0,
                self.host.surface_configuration.size().1,
            )?,
            SurfaceGeometry::Direct => unreachable!(),
        };
        self.present_geometry(camera)
    }

    /// Draw standalone Direct CPU order.
    pub(crate) fn render_cpu_sorted_indices(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.render_cpu_sorted_indices_tracked(sorted_indices, camera, refresh_indices, None)
            .map(|_| ())
    }

    pub(crate) fn render_cpu_sorted_indices_tracked(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
        completion: Option<CpuCompletionSampleRequest>,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        self.host.surface_lifecycle.begin_frame();
        if !matches!(self.geometry, SurfaceGeometry::Direct) {
            return Err(SurfacePresenterError::PagedAtlasUnsupported);
        }
        let (width, height) = self.host.surface_configuration.size();
        self.direct_runtime.prepare_cpu_order(
            &self.host.queue,
            sorted_indices,
            camera,
            width,
            height,
            refresh_indices,
        )?;
        self.present_geometry_tracked(camera, completion)
    }

    fn post_sort_gpu_order_is_prepared(&self) -> bool {
        matches!(self.geometry, SurfaceGeometry::Direct)
            && self.direct_runtime.gpu_order_is_prepared()
    }

    fn gpu_order_is_prepared(&self) -> bool {
        self.post_sort_gpu_order_is_prepared()
    }

    async fn prepare_post_sort_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        if !self.host.indirect_execution_supported {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        if !matches!(self.geometry, SurfaceGeometry::Direct) {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        if self.post_sort_gpu_order_is_prepared() {
            return Ok(());
        }
        self.direct_runtime
            .prepare_gpu_order(&self.host.device)
            .await
    }

    /// Pre-creates standalone Direct GPU-order resources outside a frame.
    pub async fn prepare_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        self.prepare_post_sort_gpu_order().await
    }

    /// Native selection remains synchronous. Web cannot block the browser
    /// event loop waiting for `pop_error_scope`, so it accepts only a
    /// candidate already published by [`Self::prepare_gpu_order`].
    pub(crate) fn prepare_direct_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        if !self.host.indirect_execution_supported {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        if self.gpu_order_is_prepared() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err(SurfacePresenterError::GpuOrderPreparationRequired)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            pollster::block_on(self.prepare_gpu_order())
        }
    }

    /// Generates and stably sorts Direct depth pairs on this presenter's GPU,
    /// then draws from the resident pair buffer in the same submission.
    pub(crate) fn render_direct_gpu_order(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
        camera_revision: u64,
        completion_started: TimerInstant,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        self.host.surface_lifecycle.begin_frame();
        if !matches!(self.geometry, SurfaceGeometry::Direct) {
            return Err(SurfacePresenterError::GpuOrderUnsupported);
        }
        let (width, height) = self.host.surface_configuration.size();
        self.direct_runtime.prepare_gpu_frame(
            &self.host.device,
            &self.host.queue,
            camera,
            width,
            height,
        )?;

        // Acquire before reserving a telemetry slot so a surface error cannot
        // strand the slot in Encoding. An unavailable drawable cannot issue a
        // formal ticket for an unpresented frame.
        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(if refresh_order {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            });
        };
        let mut telemetry_sample = self
            .direct_runtime
            .begin_gpu_order_sample(camera_revision, refresh_order)?;

        let mut encoder =
            self.host
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("gsplat-surface-direct-gpu-order-encoder"),
                });
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.direct_runtime.encode_gpu_order_draw(
            &mut encoder,
            &view,
            &self.host.queue,
            refresh_order,
            telemetry_sample.as_ref(),
        )?;

        #[cfg(not(target_arch = "wasm32"))]
        self.host
            .surface_capture
            .encode(&mut encoder, &frame.texture);
        let command_buffer = encoder.finish();
        let submitted_ticket = telemetry_sample
            .as_ref()
            .map(DirectGpuTelemetrySample::ticket);
        if let Some(sample) = telemetry_sample.take() {
            self.direct_runtime
                .arm_gpu_order_sample(&command_buffer, sample, completion_started);
        }
        self.host.queue.submit(Some(command_buffer));
        self.present_frame(frame);
        Ok(match (refresh_order, submitted_ticket) {
            (false, _) => TelemetrySubmission::NotRequested,
            (true, Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (true, None) => TelemetrySubmission::RingBusy,
        })
    }

    pub(crate) fn poll_gpu_order_telemetry(&mut self) -> GpuOrderTelemetryPoll {
        self.direct_runtime
            .poll_gpu_order_telemetry(&self.host.device)
    }

    pub(crate) fn poll_cpu_order_completion_telemetry(&mut self) -> CpuOrderTelemetryPoll {
        let _ = self.host.device.poll(wgpu::PollType::Poll);
        self.cpu_order_completion_telemetry.poll()
    }

    /// Waits boundedly for queue work submitted before this call. It does not
    /// acquire the Surface, encode commands, submit work, or issue telemetry.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pump_receipt_callbacks(
        &self,
        timeout: Duration,
    ) -> Result<bool, crate::RendererError> {
        self.host.pump_receipt_callbacks(timeout)
    }

    pub(crate) fn gpu_order_timestamps_enabled(&self) -> bool {
        self.direct_runtime.gpu_order_timestamps_enabled()
    }

    fn present_geometry(&mut self, camera: &Camera) -> Result<(), SurfacePresenterError> {
        self.present_geometry_tracked(camera, None).map(|_| ())
    }

    fn present_geometry_tracked(
        &mut self,
        _camera: &Camera,
        completion: Option<CpuCompletionSampleRequest>,
    ) -> Result<TelemetrySubmission, SurfacePresenterError> {
        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(if completion.is_some() {
                TelemetrySubmission::SurfaceUnavailable
            } else {
                TelemetrySubmission::NotRequested
            });
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.host
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("gsplat-surface-encoder"),
                });
        match &self.geometry {
            SurfaceGeometry::Direct => self.direct_runtime.encode_cpu_draw(&mut encoder, &view)?,
            SurfaceGeometry::Paged(paged) => encode_splat_draw_into(
                &mut encoder,
                &SplatDraw {
                    pass_label: "gsplat-surface-paged-pass",
                    view: &view,
                    pipeline: &self.paged_pipeline,
                    bind_group: &paged.active_set.atlas.resources.bind_group,
                    clear: wgpu::Color::BLACK,
                    vertex_count: QUAD_VERTEX_COUNT,
                    instance_count: self.paged_instance_count,
                },
            ),
        }
        let instance_count = self.instance_count();
        let mut completion_ticket = completion.and_then(|request| {
            self.cpu_order_completion_telemetry
                .begin_sample_with_counts(
                    request.camera_revision,
                    request.preprocess_ms,
                    request.sort_ms,
                    FrameInstanceCounts {
                        candidate_visible: instance_count,
                        contributor: instance_count,
                        drawn: instance_count,
                        exact_contributor_compaction: false,
                    },
                )
        });
        #[cfg(not(target_arch = "wasm32"))]
        self.host
            .surface_capture
            .encode(&mut encoder, &frame.texture);
        let command_buffer = encoder.finish();
        let submitted_ticket = completion_ticket.as_ref().map(|ticket| ticket.ticket);
        if let (Some(ticket), Some(request)) = (completion_ticket.take(), completion) {
            self.cpu_order_completion_telemetry
                .arm(&command_buffer, ticket, request.started);
        }
        self.host.queue.submit(Some(command_buffer));
        self.present_frame(frame);
        Ok(match (completion, submitted_ticket) {
            (None, _) => TelemetrySubmission::NotRequested,
            (Some(_), Some(ticket)) => TelemetrySubmission::Issued(ticket),
            (Some(_), None) => TelemetrySubmission::RingBusy,
        })
    }

    fn acquire_surface_texture(
        &mut self,
    ) -> Result<Option<wgpu::SurfaceTexture>, SurfacePresenterError> {
        self.host.surface_lifecycle.acquire(
            &self.host.surface,
            &self.host.device,
            &self.host.surface_configuration,
        )
    }

    fn present_frame(&mut self, frame: wgpu::SurfaceTexture) {
        self.host.surface_lifecycle.present(frame);
        #[cfg(not(target_arch = "wasm32"))]
        self.host.surface_capture.mark_presented();
    }

    pub const fn instance_count(&self) -> u32 {
        match &self.geometry {
            SurfaceGeometry::Direct => self.direct_runtime.instance_count(),
            SurfaceGeometry::Paged(_) => self.paged_instance_count,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_surface_capture_allows_resize(pending: bool) -> Result<(), SurfacePresenterError> {
    if pending {
        return Err(SurfacePresenterError::SurfaceCaptureState(
            "cannot resize while a capture is pending".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_geometry_admission_is_structured_and_repeatable() {
        assert_eq!(
            include_str!("surface_presenter.rs")
                .matches(concat!(
                    "Self::admit_standalone_geometry(",
                    "renderer.geometry_path())?;"
                ))
                .count(),
            3,
            "every public from_* family member must admit before delegating to host/graph construction"
        );
        for path in [
            GeometryPath::SortedIndexDirect,
            GeometryPath::PagedActiveAtlas,
        ] {
            assert!(SurfacePresenter::admit_standalone_geometry(path).is_ok());
        }

        for _ in 0..2 {
            let error = SurfacePresenter::admit_standalone_geometry(GeometryPath::PackedAtlas)
                .expect_err("standalone Packed admission");
            assert!(matches!(
                error,
                SurfacePresenterError::StandalonePackedPresenterUnsupported
            ));
            assert_eq!(error.code(), gsplat_core::ErrorCode::Unsupported);
        }
    }

    #[test]
    fn standalone_presenter_source_contains_no_packed_graph_resources() {
        let source = include_str!("surface_presenter.rs");
        for removed in [
            concat!("Surface", "PackedRuntime"),
            concat!("SurfaceGeometry::", "Packed"),
            concat!("ProjectedQuads", "Gpu"),
            concat!("Preprojected", "GpuOrder"),
            concat!("ProjectedDrawTelemetry::", "new"),
            concat!("GpuProducerTelemetry::", "new"),
            concat!("create_resident_draw_", "pipeline"),
            concat!("create_resident_color_", "pipeline"),
            concat!("GpuOrderTelemetry::", "new"),
            concat!("create_direct_", "pipeline"),
            concat!("create_direct_", "bind_group_layout"),
            concat!("DirectScene", "Resources"),
            concat!("GpuOrderTimestamp", "Range"),
            concat!("SplatIndirect", "Draw"),
        ] {
            assert!(
                !source.contains(removed),
                "standalone presenter retained legacy Packed graph resource {removed}"
            );
        }

        let crate_root = include_str!("lib.rs");
        assert!(!crate_root.contains(concat!("mod projected_", "quads_gpu;")));

        let preproject = include_str!("preproject_gpu.rs");
        assert!(!preproject.contains(concat!("Preprojected", "GpuOrder")));
        assert!(!preproject.contains(concat!("mod ", "raster;")));

        for telemetry in [
            include_str!("projected_draw_telemetry.rs"),
            include_str!("gpu_producer_telemetry.rs"),
        ] {
            assert!(!telemetry.contains("wgpu::"));
            assert!(!telemetry.contains("create_buffer"));
            assert!(!telemetry.contains("map_buffer_on_submit"));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pending_surface_capture_blocks_resize() {
        assert!(ensure_surface_capture_allows_resize(false).is_ok());
        assert!(matches!(
            ensure_surface_capture_allows_resize(true),
            Err(SurfacePresenterError::SurfaceCaptureState(message))
                if message == "cannot resize while a capture is pending"
        ));
    }

    #[test]
    fn gpu_order_requires_indirect_execution_before_resource_creation() {
        let supported = wgpu::DownlevelCapabilities::default();
        assert!(supports_direct_gpu_order(&supported));

        let mut unsupported = supported;
        unsupported
            .flags
            .remove(wgpu::DownlevelFlags::INDIRECT_EXECUTION);
        assert!(!supports_direct_gpu_order(&unsupported));
    }

    fn adapter_limits(storage_bytes: u32, buffer_bytes: u64) -> wgpu::Limits {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_storage_buffer_binding_size = storage_bytes;
        adapter_limits.max_buffer_size = buffer_bytes;
        adapter_limits.max_storage_buffers_per_shader_stage = 8;
        adapter_limits
    }

    fn resource_plan(
        path: GeometryPath,
        scene_splats: usize,
        sh_degree: u8,
        page_count: usize,
        page_capacity: usize,
        limits: &wgpu::Limits,
    ) -> SurfaceResourcePlan {
        surface_resource_plan(
            path,
            scene_splats,
            sh_degree,
            page_count,
            page_capacity,
            716,
            1_600,
            limits,
        )
        .expect("resource plan")
    }

    #[test]
    fn small_direct_scene_does_not_request_obsolete_packed_headroom() {
        let mut adapter = adapter_limits(256 << 20, 512 << 20);
        adapter.max_texture_dimension_2d = 16_384;
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 279_199, 3, 0, 0, &adapter);

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
        assert_eq!(
            requested.max_storage_buffers_per_shader_stage,
            wgpu::Limits::downlevel_defaults().max_storage_buffers_per_shader_stage,
            "standalone Direct must not reserve bindings for the deleted Packed graph"
        );
    }

    #[test]
    fn direct_device_without_resident_bindings_still_constructs_direct_limits() {
        let mut adapter = adapter_limits(256 << 20, 512 << 20);
        adapter.max_storage_buffers_per_shader_stage =
            resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS - 1;
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 279_199, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("direct limits");

        assert_eq!(
            requested.max_storage_buffers_per_shader_stage,
            wgpu::Limits::downlevel_defaults().max_storage_buffers_per_shader_stage
        );
    }

    #[test]
    fn larger_adapter_requests_exact_750k_degree_three_binding() {
        let adapter = adapter_limits(256 << 20, 512 << 20);
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 750_000, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffer_binding_size, 135_000_000);
        assert_eq!(requested.max_buffer_size, 256 << 20);
        assert_eq!(
            crate::direct_scene_preflight(750_000, 3, &requested)
                .expect("preflight")
                .path,
            crate::DirectScenePath::Direct
        );
    }

    #[test]
    fn surface_larger_than_adapter_texture_limit_is_rejected() {
        let adapter = adapter_limits(256 << 20, 512 << 20);
        let plan = surface_resource_plan(
            GeometryPath::SortedIndexDirect,
            279_199,
            3,
            0,
            0,
            adapter.max_texture_dimension_2d + 1,
            1_600,
            &adapter,
        )
        .expect("resource plan");

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();

        assert!(matches!(error, SurfacePresenterError::DeviceCreation(_)));
        assert!(error.to_string().contains("required texture dimension"));
    }

    #[test]
    fn physical_128_mib_adapter_rejects_750k_degree_three_scene() {
        let adapter = wgpu::Limits::downlevel_defaults();
        let plan = resource_plan(GeometryPath::SortedIndexDirect, 750_000, 3, 0, 0, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::DirectScene(DirectSceneError::ResourceLimitExceeded(report)) =
            error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(report.effective_storage_binding_limit, 128 << 20);
        assert_eq!(report.max_direct_splats, 745_654);
        assert_eq!(report.requirements[2].required_bytes, 135_000_000);
    }

    #[test]
    fn direct_scene_above_256_mib_raises_binding_and_buffer_exactly() {
        let adapter = adapter_limits(512 << 20, 512 << 20);
        let plan = resource_plan(
            GeometryPath::SortedIndexDirect,
            1_500_000,
            3,
            0,
            0,
            &adapter,
        );

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(requested.max_storage_buffer_binding_size, 270_000_000);
        assert_eq!(requested.max_buffer_size, 270_000_000);
    }

    #[test]
    fn packed_selection_does_not_request_oversized_direct_binding() {
        let adapter = adapter_limits(512 << 20, 512 << 20);
        let plan = resource_plan(GeometryPath::PackedAtlas, 3_000_000, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(
            plan.direct_preflight.path,
            DirectScenePath::ActiveAtlasRequired
        );
        assert_eq!(plan.packed_preflight.path, PackedScenePath::PackedAtlas);
        assert_eq!(
            plan.packed_preflight.resident_gpu.position_alpha,
            48_000_000
        );
        assert_eq!(plan.packed_preflight.resident_gpu.sh_plane_count, 4);
        assert_eq!(
            plan.packed_preflight.largest_storage_binding_bytes,
            48_000_000
        );
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
    fn paged_selection_requests_only_fixed_resident_capacity() {
        let adapter = wgpu::Limits::downlevel_defaults();
        let page_capacity = 65_536;
        let scene_splats = 10_000_000;
        let plan = resource_plan(
            GeometryPath::PagedActiveAtlas,
            scene_splats,
            3,
            scene_splats.div_ceil(page_capacity),
            page_capacity,
            &adapter,
        );

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(plan.paged_plan.hot_record_storage_bytes, 4 * 65_536 * 20);
        assert_eq!(plan.packed_preflight.path, PackedScenePath::PagingRequired);
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
    fn packed_surface_accepts_exact_portable_binding_boundary() {
        let adapter = adapter_limits(128 << 20, 256 << 20);
        let plan = resource_plan(GeometryPath::PackedAtlas, 8_388_608, 3, 0, 0, &adapter);

        let requested = surface_required_device_limits(&adapter, &plan).expect("limits");

        assert_eq!(plan.packed_preflight.path, PackedScenePath::PackedAtlas);
        assert_eq!(
            plan.resident_plan.largest_storage_binding_bytes(),
            128 << 20
        );
        assert_eq!(requested.max_storage_buffer_binding_size, 128 << 20);
        assert_eq!(requested.max_buffer_size, 256 << 20);
        assert_eq!(requested.max_storage_buffers_per_shader_stage, 8);
    }

    #[test]
    fn packed_surface_rejects_one_splat_above_portable_binding_boundary() {
        let adapter = adapter_limits(128 << 20, 256 << 20);
        let plan = resource_plan(GeometryPath::PackedAtlas, 8_388_609, 3, 0, 0, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::DirectScene(DirectSceneError::PackedResourceLimitExceeded(
            report,
        )) = error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(report.largest_storage_binding_bytes, (128 << 20) + 16);
        assert_eq!(
            report.failure,
            Some(crate::PackedScenePreflightFailure::StorageBindingSize {
                required_bytes: (128 << 20) + 16,
                limit_bytes: 128 << 20,
            })
        );
    }

    #[test]
    fn packed_surface_rejects_fewer_than_eight_storage_bindings() {
        let mut adapter = adapter_limits(128 << 20, 256 << 20);
        adapter.max_storage_buffers_per_shader_stage = 7;
        let plan = resource_plan(GeometryPath::PackedAtlas, 1, 3, 0, 0, &adapter);

        let error = surface_required_device_limits(&adapter, &plan).unwrap_err();
        let SurfacePresenterError::DirectScene(DirectSceneError::PackedResourceLimitExceeded(
            report,
        )) = error
        else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(
            report.failure,
            Some(crate::PackedScenePreflightFailure::StorageBindingCount {
                required: 8,
                available: 7,
            })
        );
    }
}
