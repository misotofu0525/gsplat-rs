//! WGPU Surface presentation and geometry-resource ownership.

use gsplat_core::{Camera, SceneBuffers};
use gsplat_sort::CpuSortBackend;

use crate::draw_pass::{SplatDraw, encode_splat_draw};
use crate::packed_gpu;
use crate::paged_active_set::PagedActiveSet;
use crate::{
    DEFAULT_PAGED_ATLAS_SLOTS, DirectSceneError, DirectScenePath, DirectScenePreflight,
    DirectSceneResources, GeometryPath, PackedScenePath, PackedScenePreflight, Renderer,
    SpatialPageSet, SurfacePresenterError, create_direct_bind_group_layout, create_direct_pipeline,
    create_surface_instance, direct_scene_preflight, fit_surface_size,
    packed_color_refresh_band_size, packed_color_refresh_needed, packed_color_refresh_position_key,
    packed_scene_preflight, preprocess_paged_visible_into, refresh_packed_hot_colors_range,
    refresh_paged_hot_colors, select_automatic_surface_geometry_path, select_present_mode,
    surface_error_to_presenter, wgpu_label,
};

enum SurfacePathSelection<'a> {
    Exact(&'a Renderer),
    Automatic(&'a mut Renderer),
}

struct SurfaceAdapterContext {
    info: wgpu::AdapterInfo,
    limits: wgpu::Limits,
    effective_device_limits: wgpu::Limits,
}

fn restore_renderer_path_on_error<T, E>(
    renderer: &mut Renderer,
    previous_path: GeometryPath,
    result: Result<T, E>,
) -> Result<T, E> {
    if result.is_err() {
        renderer.set_geometry_path(previous_path);
    }
    result
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
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        pages: SpatialPageSet,
    ) -> Result<Self, SurfacePresenterError> {
        let active_set = PagedActiveSet::new(device, queue, layout, scene, pages)
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
    /// Last camera position whose hot colors have been fully applied.
    packed_color_refresh_position: Option<[f32; 3]>,
    /// Next splat index for an in-flight banded refresh, if any.
    packed_color_refresh_cursor: Option<usize>,
    /// Camera position the in-flight banded refresh is converging toward.
    packed_color_refresh_target: Option<[f32; 3]>,
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
    queue: &wgpu::Queue,
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
                queue,
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
                SurfacePagedRuntime::new(device, queue, packed_bind_group_layout, scene, pages)?;
            Ok(SurfaceGeometry::Paged(Box::new(paged_scene)))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceResourcePlan {
    pub(crate) geometry_path: GeometryPath,
    pub(crate) scene_splats: usize,
    pub(crate) page_count: usize,
    pub(crate) slot_count: usize,
    pub(crate) resident_capacity: usize,
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
    let (slot_count, resident_capacity, packed_sh_degree) = match geometry_path {
        GeometryPath::PagedActiveAtlas => {
            let slot_count = page_count.clamp(1, DEFAULT_PAGED_ATLAS_SLOTS);
            let resident_capacity = slot_count
                .checked_mul(page_capacity.max(1))
                .ok_or(DirectSceneError::ResourceSizeOverflow)?;
            (slot_count, resident_capacity, 0)
        }
        GeometryPath::SortedIndexDirect | GeometryPath::PackedAtlas => (0, scene_splats, sh_degree),
    };
    let packed_preflight = packed_scene_preflight(
        resident_capacity,
        packed_sh_degree,
        limits.max_texture_dimension_2d,
        u64::from(limits.max_storage_buffer_binding_size),
    )?;
    let required_texture_dimension = width
        .max(height)
        .max(packed_preflight.hot_atlas_width)
        .max(packed_preflight.hot_atlas_height)
        .max(packed_preflight.sh_atlas_width)
        .max(packed_preflight.sh_atlas_height);

    Ok(SurfaceResourcePlan {
        geometry_path,
        scene_splats,
        page_count,
        slot_count,
        resident_capacity,
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

fn select_automatic_surface_geometry_path_for_adapter(
    scene_splats: usize,
    sh_degree: u8,
    adapter_limits: &wgpu::Limits,
) -> Result<GeometryPath, SurfacePresenterError> {
    let effective_device_limits = surface_effective_device_limits(adapter_limits)?;
    let direct_preflight =
        direct_scene_preflight(scene_splats, sh_degree, &effective_device_limits)?;
    Ok(select_automatic_surface_geometry_path(&direct_preflight))
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
        Self::from_window_selected(target, width, height, SurfacePathSelection::Exact(renderer))
            .await
    }

    /// Creates a Surface presenter and selects Paged only when Direct cannot
    /// fit the resource limits requested from the compatible adapter.
    ///
    /// The renderer remains on its previous path when presenter preparation
    /// fails. Stable constructors do not opt into this policy implicitly.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_window_auto<T>(
        target: T,
        width: u32,
        height: u32,
        renderer: &mut Renderer,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        Self::from_window_selected(
            target,
            width,
            height,
            SurfacePathSelection::Automatic(renderer),
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn from_window_selected<T>(
        target: T,
        width: u32,
        height: u32,
        selection: SurfacePathSelection<'_>,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let instance = create_surface_instance();
        let surface = instance
            .create_surface(target)
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, selection).await
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
            SurfacePathSelection::Exact(renderer),
        ))
    }

    /// Auto-selecting counterpart to [`Self::from_raw_handles`].
    ///
    /// # Safety
    ///
    /// The raw handles must satisfy the same lifetime requirements as
    /// [`Self::from_raw_handles`].
    pub unsafe fn from_raw_handles_auto(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &mut Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        pollster::block_on(Self::from_raw_handles_selected(
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            SurfacePathSelection::Automatic(renderer),
        ))
    }

    async fn from_raw_handles_selected(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        selection: SurfacePathSelection<'_>,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let instance = create_surface_instance();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|_| SurfacePresenterError::SurfaceCreation)?;

        Self::from_surface_async(instance, surface, width, height, selection).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        Self::from_canvas_selected(canvas, width, height, SurfacePathSelection::Exact(renderer))
            .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas_auto(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        renderer: &mut Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        Self::from_canvas_selected(
            canvas,
            width,
            height,
            SurfacePathSelection::Automatic(renderer),
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    async fn from_canvas_selected(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        selection: SurfacePathSelection<'_>,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }
        let instance = create_surface_instance();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|_| SurfacePresenterError::SurfaceCreation)?;
        Self::from_surface_async(instance, surface, width, height, selection).await
    }

    async fn from_surface_async(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        mut selection: SurfacePathSelection<'_>,
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
        let previous_path = match &selection {
            SurfacePathSelection::Exact(renderer) => renderer.geometry_path(),
            SurfacePathSelection::Automatic(renderer) => renderer.geometry_path(),
        };
        if let SurfacePathSelection::Automatic(renderer) = &mut selection {
            let scene = renderer
                .scene()
                .ok_or(SurfacePresenterError::SceneNotLoaded)?;
            let selected = select_automatic_surface_geometry_path_for_adapter(
                scene.len(),
                scene.sh_degree,
                &adapter_context.limits,
            )?;
            renderer.set_geometry_path(selected);
        }
        let renderer = match &selection {
            SurfacePathSelection::Exact(renderer) => *renderer,
            SurfacePathSelection::Automatic(renderer) => &**renderer,
        };
        let result = Self::from_surface_with_adapter_async(
            adapter,
            surface,
            width,
            height,
            renderer,
            adapter_context,
        )
        .await;
        if let SurfacePathSelection::Automatic(renderer) = &mut selection {
            return restore_renderer_path_on_error(renderer, previous_path, result);
        }
        result
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
        // Surface sessions support runtime direct↔packed A/B switching, so the
        // device must negotiate the loaded scene's packed sidecar requirement
        // even when the presenter is initially created on the Direct path.
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
            &queue,
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
            packed_color_refresh_position: None,
            packed_color_refresh_cursor: None,
            packed_color_refresh_target: None,
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
                presenter.packed_color_refresh_position = None;
                presenter.packed_color_refresh_cursor = None;
                presenter.packed_color_refresh_target = None;
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
            &self.queue,
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
        match self.geometry.path() {
            GeometryPath::SortedIndexDirect => {
                self.render_direct_frame(sorted_indices, camera, refresh_indices)
            }
            GeometryPath::PackedAtlas => {
                self.render_packed_frame(scene, sorted_indices, camera, refresh_indices)
            }
            GeometryPath::PagedActiveAtlas => self.render_paged_frame(scene, camera).map(|_| ()),
        }
    }

    fn render_direct_frame(
        &mut self,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        let SurfaceGeometry::Direct(direct) = &self.geometry else {
            return Err(SurfacePresenterError::SceneNotLoaded);
        };
        self.instance_count = direct.prepare(
            &self.queue,
            sorted_indices,
            camera,
            self.surface_config.width,
            self.surface_config.height,
            refresh_indices,
        )?;
        self.present_geometry()
    }

    fn render_packed_frame(
        &mut self,
        scene: &SceneBuffers,
        sorted_indices: &[u32],
        camera: &Camera,
        refresh_indices: bool,
    ) -> Result<(), SurfacePresenterError> {
        let position_key = packed_color_refresh_position_key(camera);
        let SurfaceGeometry::Packed(packed) = &self.geometry else {
            return Err(SurfacePresenterError::SceneNotLoaded);
        };
        let needs_refresh = packed_color_refresh_needed(
            self.packed_color_refresh_position,
            camera,
            packed.bounds_min,
            packed.bounds_extent,
        );
        if !refresh_indices && (self.packed_color_refresh_cursor.is_some() || needs_refresh) {
            // Defer banded SH refresh off synchronous sort frames so p95 does
            // not stack a full CPU sort with SH eval/upload.
            if self.packed_color_refresh_cursor.is_none() {
                self.packed_color_refresh_cursor = Some(0);
                self.packed_color_refresh_target = Some(position_key);
            }
            let start = self.packed_color_refresh_cursor.unwrap_or(0);
            let end = (start + packed_color_refresh_band_size(scene.len())).min(scene.len());
            let SurfaceGeometry::Packed(packed) = &mut self.geometry else {
                return Err(SurfacePresenterError::SceneNotLoaded);
            };
            refresh_packed_hot_colors_range(&self.queue, packed, scene, camera, start, end);
            if end >= scene.len() {
                self.packed_color_refresh_position = self.packed_color_refresh_target;
                self.packed_color_refresh_cursor = None;
                self.packed_color_refresh_target = None;
            } else {
                self.packed_color_refresh_cursor = Some(end);
            }
        }

        let SurfaceGeometry::Packed(packed) = &self.geometry else {
            return Err(SurfacePresenterError::SceneNotLoaded);
        };
        self.instance_count = packed.prepare(
            &self.queue,
            sorted_indices,
            camera,
            self.surface_config.width,
            self.surface_config.height,
            refresh_indices,
        )?;
        self.present_geometry()
    }

    fn render_paged_frame(
        &mut self,
        scene: &SceneBuffers,
        camera: &Camera,
    ) -> Result<u32, SurfacePresenterError> {
        let SurfaceGeometry::Paged(paged) = &mut self.geometry else {
            return Err(SurfacePresenterError::SceneNotLoaded);
        };
        self.instance_count = paged.prepare(
            &self.queue,
            scene,
            camera,
            self.surface_config.width,
            self.surface_config.height,
        )?;
        self.present_geometry()?;
        Ok(self.instance_count)
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
                &direct.bind_group,
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
    use gsplat_core::RenderMode;

    use super::*;

    #[test]
    fn automatic_selection_failure_restores_the_previous_renderer_path() {
        let mut renderer = Renderer::new_for_surface(RenderMode::SortedAlpha).unwrap();
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        let previous_path = renderer.geometry_path();
        renderer.set_geometry_path(GeometryPath::PagedActiveAtlas);
        let result = restore_renderer_path_on_error(
            &mut renderer,
            previous_path,
            Err::<(), _>("surface preparation failed"),
        );
        assert_eq!(result, Err("surface preparation failed"));
        assert_eq!(renderer.geometry_path(), GeometryPath::PackedAtlas);
    }

    #[test]
    fn automatic_selection_uses_effective_requested_storage_limits() {
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

        let selected = super::select_automatic_surface_geometry_path_for_adapter(
            3_000_000,
            0,
            &adapter_limits,
        )
        .unwrap();

        assert_eq!(selected, crate::GeometryPath::PagedActiveAtlas);
    }
}
