//! WGPU Surface presentation and geometry-resource ownership.

use gsplat_core::{Camera, SceneBuffers};
use gsplat_sort::CpuSortBackend;

use crate::draw_pass::{SplatDraw, encode_splat_draw, encode_splat_draw_into};
use crate::packed_gpu;
use crate::paged_active_set::PagedActiveSet;
use crate::{
    DEFAULT_PAGED_ATLAS_SLOTS, DirectSceneError, DirectScenePath, DirectScenePreflight,
    DirectSceneResources, GeometryPath, PackedScenePath, PackedScenePreflight, Renderer,
    SpatialPageSet, SurfacePresenterError, create_direct_bind_group_layout, create_direct_pipeline,
    create_surface_instance, direct_scene_preflight, fit_surface_size,
    packed_color_refresh_band_size, packed_color_refresh_needed, packed_color_refresh_position_key,
    packed_scene_preflight, preprocess_paged_visible_into, refresh_packed_hot_colors,
    refresh_packed_hot_colors_range, refresh_paged_hot_colors, select_present_mode,
    surface_error_to_presenter, wgpu_label,
};

struct SurfaceAdapterContext {
    info: wgpu::AdapterInfo,
    limits: wgpu::Limits,
    effective_device_limits: wgpu::Limits,
}

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

pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    direct_pipeline: wgpu::RenderPipeline,
    direct_bind_group_layout: wgpu::BindGroupLayout,
    packed_pipeline: wgpu::RenderPipeline,
    packed_bind_group_layout: wgpu::BindGroupLayout,
    surface_config: wgpu::SurfaceConfiguration,
    max_texture_dimension_2d: u32,
    instance_count: u32,
    geometry: SurfaceGeometry,
    packed_color_refresh: PackedColorRefreshState,
}

#[derive(Debug, Default)]
struct PackedColorRefreshState {
    /// Last camera position whose hot colors have been fully applied.
    applied_position: Option<[f32; 3]>,
    /// Next splat index for an in-flight banded refresh, if any.
    cursor: Option<usize>,
    /// Frozen camera for the whole in-flight refresh.
    target_camera: Option<Camera>,
}

impl PackedColorRefreshState {
    fn needs_full_refresh(&self) -> bool {
        self.applied_position.is_none()
    }

    fn mark_full_refresh(&mut self, camera: &Camera) {
        self.applied_position = Some(packed_color_refresh_position_key(camera));
        self.cursor = None;
        self.target_camera = None;
    }

    fn begin_banded_refresh(&mut self, camera: &Camera) {
        debug_assert!(self.cursor.is_none());
        self.cursor = Some(0);
        self.target_camera = Some(*camera);
    }

    fn batch(&self, splat_count: usize) -> Option<(usize, usize, Camera)> {
        let start = self.cursor?;
        let target = self.target_camera?;
        let end = (start + packed_color_refresh_band_size(splat_count)).min(splat_count);
        Some((start, end, target))
    }

    fn finish_batch(&mut self, end: usize, splat_count: usize) {
        if end >= splat_count {
            if let Some(target) = self.target_camera {
                self.mark_full_refresh(&target);
            }
        } else {
            self.cursor = Some(end);
        }
    }
}

enum SurfaceGeometry {
    Direct(DirectSceneResources),
    Packed(packed_gpu::PackedAtlasResources),
    Paged(Box<SurfacePagedRuntime>),
}

impl SurfaceGeometry {
    const fn path(&self) -> GeometryPath {
        match self {
            Self::Direct(_) => GeometryPath::SortedIndexDirect,
            Self::Packed(_) => GeometryPath::PackedAtlas,
            Self::Paged(_) => GeometryPath::PagedActiveAtlas,
        }
    }
}

fn create_geometry_resources(
    device: &wgpu::Device,
    direct_bind_group_layout: &wgpu::BindGroupLayout,
    packed_bind_group_layout: &wgpu::BindGroupLayout,
    path: GeometryPath,
    renderer: &Renderer,
) -> Result<SurfaceGeometry, SurfacePresenterError> {
    let scene = renderer
        .scene()
        .ok_or(SurfacePresenterError::SceneNotLoaded)?;

    match path {
        GeometryPath::SortedIndexDirect => {
            let world_covariance_terms = renderer
                .world_covariance_terms
                .as_deref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let alpha_values = renderer
                .alpha_values
                .as_deref()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let direct_scene = DirectSceneResources::new(
                device,
                direct_bind_group_layout,
                scene,
                world_covariance_terms,
                alpha_values,
            )?;
            Ok(SurfaceGeometry::Direct(direct_scene))
        }
        GeometryPath::PackedAtlas => {
            let packed_scene = packed_gpu::PackedAtlasResources::from_scene(
                device,
                packed_bind_group_layout,
                scene,
            )?;
            Ok(SurfaceGeometry::Packed(packed_scene))
        }
        GeometryPath::PagedActiveAtlas => {
            let pages = renderer
                .spatial_pages
                .clone()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let paged_scene =
                SurfacePagedRuntime::new(device, packed_bind_group_layout, scene, pages)?;
            Ok(SurfaceGeometry::Paged(Box::new(paged_scene)))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceResourcePlan {
    pub(crate) geometry_path: GeometryPath,
    pub(crate) direct_preflight: DirectScenePreflight,
    pub(crate) packed_preflight: PackedScenePreflight,
    pub(crate) required_texture_dimension: u32,
}

impl SurfaceResourcePlan {
    pub(crate) fn validate_selected_path(self) -> Result<(), DirectSceneError> {
        match self.geometry_path {
            GeometryPath::SortedIndexDirect
                if self.direct_preflight.path == DirectScenePath::ActiveAtlasRequired =>
            {
                Err(DirectSceneError::ResourceLimitExceeded(Box::new(
                    self.direct_preflight,
                )))
            }
            GeometryPath::PackedAtlas | GeometryPath::PagedActiveAtlas
                if self.packed_preflight.path == PackedScenePath::PagingRequired =>
            {
                Err(DirectSceneError::PackedResourceLimitExceeded(Box::new(
                    self.packed_preflight,
                )))
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
    let (resident_capacity, packed_sh_degree) = match geometry_path {
        GeometryPath::PagedActiveAtlas => {
            let slot_count = page_count.clamp(1, DEFAULT_PAGED_ATLAS_SLOTS);
            let resident_capacity = slot_count
                .checked_mul(page_capacity.max(1))
                .ok_or(DirectSceneError::ResourceSizeOverflow)?;
            (resident_capacity, 0)
        }
        GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => (scene_splats, sh_degree),
    };
    let packed_preflight = packed_scene_preflight(
        resident_capacity,
        packed_sh_degree,
        u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size),
    )?;
    let required_texture_dimension = width.max(height);

    Ok(SurfaceResourcePlan {
        geometry_path,
        direct_preflight,
        packed_preflight,
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

fn surface_effective_device_limits(
    adapter_limits: &wgpu::Limits,
) -> Result<wgpu::Limits, SurfacePresenterError> {
    let mut effective_limits = wgpu::Limits::downlevel_defaults();
    // Surface resource planning may raise this field to any dimension the
    // adapter supports. Storage and buffer limits remain exactly what the
    // subsequent device descriptor will request.
    effective_limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
    if !effective_limits.check_limits(adapter_limits) {
        return Err(SurfacePresenterError::DeviceCreation(
            "surface downlevel limits exceed adapter capabilities".to_string(),
        ));
    }
    Ok(effective_limits)
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
        let effective_device_limits = surface_effective_device_limits(&adapter_limits)?;
        let adapter_context = SurfaceAdapterContext {
            info: adapter.get_info(),
            limits: adapter_limits,
            effective_device_limits,
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
            effective_device_limits,
        } = adapter_context;
        // Surface sessions support runtime direct↔packed A/B switching, so
        // preflight both storage layouts before creating the shared device.
        let scene = renderer
            .scene()
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
            scene.len(),
            scene.sh_degree,
            page_count,
            page_capacity,
            width,
            height,
            &effective_device_limits,
        )?;
        resource_plan.validate_selected_path()?;
        let required_texture_dimension = resource_plan.required_texture_dimension;
        if required_texture_dimension > adapter_limits.max_texture_dimension_2d {
            return Err(SurfacePresenterError::DeviceCreation(format!(
                "required texture dimension {required_texture_dimension} exceeds adapter limit {}",
                adapter_limits.max_texture_dimension_2d
            )));
        }
        let mut required_limits = effective_device_limits;
        required_limits.max_texture_dimension_2d = wgpu::Limits::downlevel_defaults()
            .max_texture_dimension_2d
            .max(required_texture_dimension);
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

        let direct_bind_group_layout = create_direct_bind_group_layout(&device);
        let direct_pipeline = create_direct_pipeline(&device, &direct_bind_group_layout, format);
        let packed_bind_group_layout = packed_gpu::create_packed_bind_group_layout(&device);
        let packed_pipeline =
            packed_gpu::create_packed_pipeline(&device, &packed_bind_group_layout, format);
        let geometry = create_geometry_resources(
            &device,
            &direct_bind_group_layout,
            &packed_bind_group_layout,
            geometry_path,
            renderer,
        )?;

        Ok(Self {
            surface,
            device,
            queue,
            direct_pipeline,
            direct_bind_group_layout,
            packed_pipeline,
            packed_bind_group_layout,
            surface_config,
            max_texture_dimension_2d,
            instance_count: 0,
            geometry,
            packed_color_refresh: PackedColorRefreshState::default(),
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

    pub const fn geometry_path(&self) -> GeometryPath {
        self.geometry.path()
    }

    /// Switches the Surface geometry path, clearing and rebuilding the GPU
    /// scene resources for the new path from the renderer's loaded scene.
    ///
    /// This is an experimental A/B benchmark knob: callers must keep
    /// `renderer`'s loaded scene in sync with the presenter that was created
    /// from it.
    pub fn set_geometry_path(
        &mut self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<(), SurfacePresenterError> {
        if self.geometry.path() == path {
            return Ok(());
        }

        try_prepare_then_commit(
            self,
            |presenter| presenter.prepare_geometry_resources(path, renderer),
            |presenter, geometry| {
                presenter.geometry = geometry;
                presenter.packed_color_refresh = PackedColorRefreshState::default();
                presenter.instance_count = 0;
            },
        )
    }

    fn prepare_geometry_resources(
        &self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<SurfaceGeometry, SurfacePresenterError> {
        create_geometry_resources(
            &self.device,
            &self.direct_bind_group_layout,
            &self.packed_bind_group_layout,
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
        self.instance_count = match &mut self.geometry {
            SurfaceGeometry::Direct(direct) => direct.prepare_cpu(
                &self.queue,
                sorted_indices,
                camera,
                self.surface_config.width,
                self.surface_config.height,
                refresh_indices,
            )?,
            SurfaceGeometry::Packed(packed) => {
                if self.packed_color_refresh.needs_full_refresh() {
                    // Packed records are initialized with DC only. The first
                    // presented frame must receive complete view-dependent SH
                    // colors even though it also uploads the initial order.
                    refresh_packed_hot_colors(&self.queue, packed, scene, camera);
                    self.packed_color_refresh.mark_full_refresh(camera);
                }
                let needs_refresh = packed_color_refresh_needed(
                    self.packed_color_refresh.applied_position,
                    camera,
                    packed.bounds_min,
                    packed.bounds_extent,
                );
                if !refresh_indices && (self.packed_color_refresh.cursor.is_some() || needs_refresh)
                {
                    // Defer banded SH refresh off synchronous sort frames so
                    // p95 does not stack a full CPU sort with SH eval/upload.
                    if self.packed_color_refresh.cursor.is_none() {
                        self.packed_color_refresh.begin_banded_refresh(camera);
                    }
                    if let Some((start, end, target)) = self.packed_color_refresh.batch(scene.len())
                    {
                        refresh_packed_hot_colors_range(
                            &self.queue,
                            packed,
                            scene,
                            &target,
                            start,
                            end,
                        );
                        self.packed_color_refresh.finish_batch(end, scene.len());
                    }
                }
                packed.prepare(
                    &self.queue,
                    sorted_indices,
                    camera,
                    self.surface_config.width,
                    self.surface_config.height,
                    refresh_indices,
                )?
            }
            SurfaceGeometry::Paged(paged) => paged.prepare(
                &self.queue,
                scene,
                camera,
                self.surface_config.width,
                self.surface_config.height,
            )?,
        };
        self.present_geometry()
    }

    /// Pre-creates the Direct GPU ordering pipelines and buffers outside a
    /// measured/presented frame. No sorting or drawing happens here.
    pub(crate) fn prepare_direct_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        match &mut self.geometry {
            SurfaceGeometry::Direct(direct) => {
                direct.ensure_gpu_order(&self.device, &self.direct_bind_group_layout)?
            }
            SurfaceGeometry::Packed(_) | SurfaceGeometry::Paged(_) => {
                return Err(SurfacePresenterError::GpuOrderUnsupported);
            }
        }
        Ok(())
    }

    /// Generates and stably sorts Direct depth pairs on this presenter's GPU,
    /// then draws from the resident pair buffer in the same submission.
    pub(crate) fn render_direct_gpu_order(
        &mut self,
        camera: &Camera,
        refresh_order: bool,
    ) -> Result<(), SurfacePresenterError> {
        self.instance_count = match &mut self.geometry {
            SurfaceGeometry::Direct(direct) => direct.prepare_gpu(
                &self.device,
                &self.direct_bind_group_layout,
                &self.queue,
                camera,
                self.surface_config.width,
                self.surface_config.height,
            )?,
            SurfaceGeometry::Packed(_) | SurfaceGeometry::Paged(_) => {
                return Err(SurfacePresenterError::GpuOrderUnsupported);
            }
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-direct-gpu-order-encoder"),
            });
        {
            let direct = match &self.geometry {
                SurfaceGeometry::Direct(direct) => direct,
                SurfaceGeometry::Packed(_) | SurfaceGeometry::Paged(_) => unreachable!(),
            };
            let gpu_order = direct
                .gpu_order()
                .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
            if refresh_order {
                gpu_order.sorter.encode(&mut encoder);
            }
        }

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
        let direct = match &self.geometry {
            SurfaceGeometry::Direct(direct) => direct,
            SurfaceGeometry::Packed(_) | SurfaceGeometry::Paged(_) => unreachable!(),
        };
        let gpu_order = direct
            .gpu_order()
            .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                encoder_label: "gsplat-surface-direct-gpu-order-encoder",
                pass_label: "gsplat-surface-direct-gpu-order-draw-pass",
                view: &view,
                pipeline: &self.direct_pipeline,
                bind_group: &gpu_order.bind_group,
                clear: wgpu::Color::BLACK,
                vertex_count: 6,
                instance_count: self.instance_count,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn present_geometry(&mut self) -> Result<(), SurfacePresenterError> {
        let Some(frame) = self.acquire_surface_texture()? else {
            return Ok(());
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (pipeline, bind_group, vertex_count, encoder_label, pass_label) = match &self.geometry {
            SurfaceGeometry::Direct(direct) => (
                &self.direct_pipeline,
                &direct.cpu_bind_group,
                6,
                "gsplat-surface-direct-encoder",
                "gsplat-surface-direct-pass",
            ),
            SurfaceGeometry::Packed(packed) => (
                &self.packed_pipeline,
                &packed.bind_group,
                packed_gpu::PACKED_QUAD_VERTEX_COUNT,
                "gsplat-surface-packed-encoder",
                "gsplat-surface-packed-pass",
            ),
            SurfaceGeometry::Paged(paged) => (
                &self.packed_pipeline,
                &paged.active_set.atlas.resources.bind_group,
                packed_gpu::PACKED_QUAD_VERTEX_COUNT,
                "gsplat-surface-paged-encoder",
                "gsplat-surface-paged-pass",
            ),
        };
        let commands = encode_splat_draw(
            &self.device,
            SplatDraw {
                encoder_label,
                pass_label,
                view: &view,
                pipeline,
                bind_group,
                clear: wgpu::Color::BLACK,
                vertex_count,
                instance_count: self.instance_count,
            },
        );
        self.queue.submit(Some(commands));
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
    use gsplat_core::Vec3f;

    use super::*;

    fn camera_at(x: f32) -> Camera {
        let mut camera = Camera::default();
        camera.pose.position = Vec3f::new(x, 0.0, 0.0);
        camera
    }

    #[test]
    fn packed_color_refresh_requires_a_complete_first_frame() {
        let mut state = PackedColorRefreshState::default();
        let camera = camera_at(1.0);

        assert!(state.needs_full_refresh());
        state.mark_full_refresh(&camera);
        assert!(!state.needs_full_refresh());
        assert_eq!(
            state.applied_position,
            Some(packed_color_refresh_position_key(&camera))
        );
    }

    #[test]
    fn packed_banded_color_refresh_keeps_one_frozen_camera() {
        let mut state = PackedColorRefreshState::default();
        let target = camera_at(1.0);
        let later_camera = camera_at(2.0);
        let splat_count = packed_color_refresh_band_size(100_000) * 2;

        state.begin_banded_refresh(&target);
        let mut batches = 0;
        while let Some((_, end, batch_target)) = state.batch(splat_count) {
            // A newer camera may arrive while this refresh is in flight, but
            // every remaining band must still use the original target.
            assert_eq!(batch_target, target);
            assert_ne!(batch_target, later_camera);
            state.finish_batch(end, splat_count);
            batches += 1;
        }

        assert!(batches > 1);
        assert_eq!(
            state.applied_position,
            Some(packed_color_refresh_position_key(&target))
        );
        assert!(state.cursor.is_none());
        assert!(state.target_camera.is_none());
    }

    #[test]
    fn surface_preflight_uses_effective_requested_storage_limits() {
        let mut adapter_limits = wgpu::Limits::downlevel_defaults();
        adapter_limits.max_storage_buffer_binding_size = 256 * 1024 * 1024;
        adapter_limits.max_buffer_size = 256 * 1024 * 1024;
        let adapter_preflight = crate::direct_scene_preflight(3_000_000, 0, &adapter_limits)
            .expect("adapter preflight");
        assert_eq!(adapter_preflight.path, crate::DirectScenePath::Direct);

        let effective_limits = super::surface_effective_device_limits(&adapter_limits)
            .expect("effective device limits");
        assert_eq!(
            effective_limits.max_storage_buffer_binding_size,
            wgpu::Limits::downlevel_defaults().max_storage_buffer_binding_size
        );
        let effective_preflight = crate::direct_scene_preflight(3_000_000, 0, &effective_limits)
            .expect("effective preflight");
        assert_eq!(
            effective_preflight.path,
            crate::DirectScenePath::ActiveAtlasRequired
        );
    }
}
