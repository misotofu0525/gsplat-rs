//! Stable C ABI surface for mobile wrappers.

use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use gsplat_core::{
    Camera, CameraIntrinsics, CameraPose, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR,
    GSPLAT_API_VERSION_MINOR, RenderMode, RendererConfig, Vec3f,
};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::{GpuSurfaceInstance, Renderer, SurfaceInstanceBuilder, SurfacePresenter};
use gsplat_sort::CpuSortBackend;

const SURFACE_CAMERA_MAX_PITCH: f32 = 1.45;
const SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER: f32 = 0.2;
const SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER: f32 = 20.0;
const DEFAULT_SURFACE_SORT_INTERVAL: u32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsplatConfig {
    pub width: u32,
    pub height: u32,
    pub mode: u32,
}

impl Default for GsplatConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            mode: RenderMode::SortedAlpha as u32,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsplatStats {
    pub frame_ms: f32,
    pub preprocess_ms: f32,
    pub sort_ms: f32,
    pub raster_ms: f32,
    pub visible_count: u32,
    pub drawn_count: u32,
}

impl From<FrameStats> for GsplatStats {
    fn from(stats: FrameStats) -> Self {
        Self {
            frame_ms: stats.frame_ms,
            preprocess_ms: stats.preprocess_ms,
            sort_ms: stats.sort_ms,
            raster_ms: stats.raster_ms,
            visible_count: stats.visible_count,
            drawn_count: stats.drawn_count,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GsplatCamera {
    pub position: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub vertical_fov_radians: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl Default for GsplatCamera {
    fn default() -> Self {
        let camera = Camera::default();
        Self {
            position: [
                camera.pose.position.x,
                camera.pose.position.y,
                camera.pose.position.z,
            ],
            rotation_xyzw: camera.pose.rotation_xyzw,
            vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
            near_plane: camera.intrinsics.near_plane,
            far_plane: camera.intrinsics.far_plane,
        }
    }
}

impl From<GsplatCamera> for Camera {
    fn from(value: GsplatCamera) -> Self {
        Self {
            pose: CameraPose {
                position: Vec3f::new(value.position[0], value.position[1], value.position[2]),
                rotation_xyzw: value.rotation_xyzw,
            },
            intrinsics: CameraIntrinsics {
                vertical_fov_radians: value.vertical_fov_radians,
                near_plane: value.near_plane,
                far_plane: value.far_plane,
            },
        }
    }
}

pub struct GsplatContext {
    renderer: Renderer,
    camera: Camera,
}

pub struct GsplatSurfaceRenderer {
    renderer: Renderer,
    presenter: SurfacePresenter,
    instances: Vec<GpuSurfaceInstance>,
    camera: Camera,
    camera_control: SurfaceCameraControl,
    surface_stats: FrameStats,
    uploaded_frame: bool,
    surface_sort_interval: u32,
    surface_frames_since_sort: u32,
    surface_gpu_preproject: bool,
    surface_gpu_preproject_double_buffer: bool,
    surface_static_direct: bool,
    surface_async_sort: bool,
    surface_async_geometry: bool,
    async_sorter: SurfaceAsyncSorter,
    async_geometry: Option<SurfaceGeometryWorker>,
    render_error_logged: bool,
}

struct SurfaceAsyncSorter {
    positions: Arc<[Vec3f]>,
    in_flight: Option<JoinHandle<Result<AsyncSortResult, ErrorCode>>>,
}

struct AsyncSortResult {
    indices: Vec<u32>,
    preprocess_ms: f32,
    sort_ms: f32,
}

struct SurfaceGeometryWorker {
    builder: Arc<SurfaceInstanceBuilder>,
    in_flight: Option<JoinHandle<Result<AsyncGeometryResult, ErrorCode>>>,
}

struct AsyncGeometryResult {
    instances: Vec<GpuSurfaceInstance>,
    build_ms: f32,
}

impl SurfaceAsyncSorter {
    fn new(scene: &gsplat_core::SceneBuffers) -> Self {
        Self {
            positions: Arc::from(scene.positions.clone().into_boxed_slice()),
            in_flight: None,
        }
    }

    fn is_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    fn poll_result(&mut self) -> Option<Result<AsyncSortResult, ErrorCode>> {
        let handle = self.in_flight.as_ref()?;
        if !handle.is_finished() {
            return None;
        }

        let handle = self.in_flight.take()?;
        Some(match handle.join() {
            Ok(result) => result,
            Err(_) => Err(ErrorCode::Internal),
        })
    }

    fn start(&mut self, camera: Camera) {
        if self.in_flight.is_some() {
            return;
        }

        let positions = Arc::clone(&self.positions);
        self.in_flight = Some(thread::spawn(move || {
            sort_positions_for_camera(&positions, camera)
        }));
    }

    fn discard_in_flight(&mut self) {
        self.in_flight = None;
    }
}

impl SurfaceGeometryWorker {
    fn new(builder: SurfaceInstanceBuilder) -> Self {
        Self {
            builder: Arc::new(builder),
            in_flight: None,
        }
    }

    fn is_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    fn poll_result(&mut self) -> Option<Result<AsyncGeometryResult, ErrorCode>> {
        let handle = self.in_flight.as_ref()?;
        if !handle.is_finished() {
            return None;
        }

        let handle = self.in_flight.take()?;
        Some(match handle.join() {
            Ok(result) => result,
            Err(_) => Err(ErrorCode::Internal),
        })
    }

    fn start(&mut self, camera: Camera, config: RendererConfig, indices: Vec<u32>) {
        if self.in_flight.is_some() {
            return;
        }

        let builder = Arc::clone(&self.builder);
        self.in_flight = Some(thread::spawn(move || {
            let build_start = Instant::now();
            let mut by_index = Vec::new();
            let mut instances = Vec::new();
            builder
                .build_into(&indices, &camera, config, &mut by_index, &mut instances)
                .map_err(|err| err.code())?;
            Ok(AsyncGeometryResult {
                instances,
                build_ms: build_start.elapsed().as_secs_f32() * 1000.0,
            })
        }));
    }

    fn discard_in_flight(&mut self) {
        self.in_flight = None;
    }
}

#[derive(Debug, Clone, Copy)]
struct SurfaceCameraControl {
    target: Vec3f,
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

fn log_surface_error(message: &str) {
    eprintln!("gsplat-ffi-c: {message}");
}

fn static_cstr(bytes: &'static [u8]) -> *const c_char {
    bytes.as_ptr() as *const c_char
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_version_major() -> u32 {
    GSPLAT_API_VERSION_MAJOR
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_version_minor() -> u32 {
    GSPLAT_API_VERSION_MINOR
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_error_message(code: i32) -> *const c_char {
    match code {
        0 => static_cstr(b"ok\0"),
        1 => static_cstr(b"invalid argument\0"),
        2 => static_cstr(b"not found\0"),
        3 => static_cstr(b"parse failed\0"),
        4 => static_cstr(b"unsupported\0"),
        5 => static_cstr(b"scene not loaded\0"),
        100 => static_cstr(b"internal error\0"),
        _ => static_cstr(b"unknown error\0"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_config_default() -> GsplatConfig {
    GsplatConfig::default()
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_camera_default() -> GsplatCamera {
    GsplatCamera::default()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_create(
    config: GsplatConfig,
    out_ctx: *mut *mut GsplatContext,
) -> i32 {
    if out_ctx.is_null() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    unsafe {
        *out_ctx = std::ptr::null_mut();
    }

    if config.mode != RenderMode::SortedAlpha as u32 {
        return ErrorCode::InvalidArgument.as_i32();
    }

    let renderer_config = RendererConfig {
        width: config.width,
        height: config.height,
        mode: RenderMode::SortedAlpha,
    };

    let renderer = match Renderer::with_config(renderer_config) {
        Ok(renderer) => renderer,
        Err(err) => return err.code().as_i32(),
    };

    let context = Box::new(GsplatContext {
        renderer,
        camera: Camera::default(),
    });

    unsafe {
        *out_ctx = Box::into_raw(context);
    }

    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_destroy(ctx: *mut GsplatContext) {
    if ctx.is_null() {
        return;
    }

    unsafe {
        drop(Box::from_raw(ctx));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_set_camera(
    ctx: *mut GsplatContext,
    camera: GsplatCamera,
) -> i32 {
    let ctx = match unsafe { ctx.as_mut() } {
        Some(ctx) => ctx,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let camera: Camera = camera.into();
    if camera.validate().is_err() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    ctx.camera = camera;
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_set_auto_camera(ctx: *mut GsplatContext) -> i32 {
    let ctx = match unsafe { ctx.as_mut() } {
        Some(ctx) => ctx,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    match auto_camera(&ctx.renderer) {
        Ok(camera) => {
            ctx.camera = camera;
            ErrorCode::Ok.as_i32()
        }
        Err(code) => code.as_i32(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_load_scene_path(
    ctx: *mut GsplatContext,
    path: *const c_char,
) -> i32 {
    let ctx = match unsafe { ctx.as_mut() } {
        Some(ctx) => ctx,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if path.is_null() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(path) => path,
        Err(_) => return ErrorCode::InvalidArgument.as_i32(),
    };

    let loaded = match load_ply(Path::new(path_str)) {
        Ok(result) => result,
        Err(err) => return err.code().as_i32(),
    };

    match ctx.renderer.load_scene(loaded.scene) {
        Ok(()) => ErrorCode::Ok.as_i32(),
        Err(err) => err.code().as_i32(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_render_frame(ctx: *mut GsplatContext) -> i32 {
    let ctx = match unsafe { ctx.as_mut() } {
        Some(ctx) => ctx,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    match ctx.renderer.render_frame(&ctx.camera) {
        Ok(_) => ErrorCode::Ok.as_i32(),
        Err(err) => err.code().as_i32(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_get_stats(
    ctx: *const GsplatContext,
    out_stats: *mut GsplatStats,
) -> i32 {
    let ctx = match unsafe { ctx.as_ref() } {
        Some(ctx) => ctx,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if out_stats.is_null() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    unsafe {
        *out_stats = ctx.renderer.last_stats().into();
    }

    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_android(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    if native_window.is_null() || path.is_null() || out_renderer.is_null() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    unsafe {
        *out_renderer = std::ptr::null_mut();
    }

    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(path) => path,
        Err(_) => return ErrorCode::InvalidArgument.as_i32(),
    };

    let mut renderer = match Renderer::with_config(RendererConfig {
        width,
        height,
        mode: RenderMode::SortedAlpha,
    }) {
        Ok(renderer) => renderer,
        Err(err) => return err.code().as_i32(),
    };

    let loaded = match load_ply(Path::new(path_str)) {
        Ok(result) => result,
        Err(err) => return err.code().as_i32(),
    };
    if let Err(err) = renderer.load_scene(loaded.scene) {
        return err.code().as_i32();
    };

    let window = match NonNull::new(native_window) {
        Some(window) => window,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };
    let raw_display_handle =
        wgpu::rwh::RawDisplayHandle::Android(wgpu::rwh::AndroidDisplayHandle::new());
    let raw_window_handle =
        wgpu::rwh::RawWindowHandle::AndroidNdk(wgpu::rwh::AndroidNdkWindowHandle::new(window));

    create_surface_renderer_from_raw_handles(
        renderer,
        raw_display_handle,
        raw_window_handle,
        width,
        height,
        out_renderer,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_uikit(
    ui_view: *mut c_void,
    ui_view_controller: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    if ui_view.is_null() || path.is_null() || out_renderer.is_null() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    unsafe {
        *out_renderer = std::ptr::null_mut();
    }

    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(path) => path,
        Err(_) => return ErrorCode::InvalidArgument.as_i32(),
    };

    let mut renderer = match Renderer::with_config(RendererConfig {
        width,
        height,
        mode: RenderMode::SortedAlpha,
    }) {
        Ok(renderer) => renderer,
        Err(err) => return err.code().as_i32(),
    };

    let loaded = match load_ply(Path::new(path_str)) {
        Ok(result) => result,
        Err(err) => return err.code().as_i32(),
    };
    if let Err(err) = renderer.load_scene(loaded.scene) {
        return err.code().as_i32();
    };

    let view = match NonNull::new(ui_view) {
        Some(view) => view,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };
    let mut window_handle = wgpu::rwh::UiKitWindowHandle::new(view);
    window_handle.ui_view_controller = NonNull::new(ui_view_controller);
    let raw_display_handle =
        wgpu::rwh::RawDisplayHandle::UiKit(wgpu::rwh::UiKitDisplayHandle::new());
    let raw_window_handle = wgpu::rwh::RawWindowHandle::UiKit(window_handle);

    create_surface_renderer_from_raw_handles(
        renderer,
        raw_display_handle,
        raw_window_handle,
        width,
        height,
        out_renderer,
    )
}

fn create_surface_renderer_from_raw_handles(
    mut renderer: Renderer,
    raw_display_handle: wgpu::rwh::RawDisplayHandle,
    raw_window_handle: wgpu::rwh::RawWindowHandle,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    let presenter = match unsafe {
        SurfacePresenter::from_raw_handles(
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            &renderer,
        )
    } {
        Ok(presenter) => presenter,
        Err(err) => {
            log_surface_error(&format!(
                "SurfacePresenter::from_raw_handles failed: {err:?}"
            ));
            return err.code().as_i32();
        }
    };

    let (surface_width, surface_height) = presenter.surface_size();
    if let Err(err) = renderer.set_size(surface_width, surface_height) {
        return err.code().as_i32();
    }
    let camera_control = match auto_surface_camera_control(&renderer) {
        Ok(camera_control) => camera_control,
        Err(code) => return code.as_i32(),
    };
    let camera = surface_camera_from_control(camera_control, renderer.config());
    let async_sorter = match renderer.scene() {
        Some(scene) => SurfaceAsyncSorter::new(scene),
        None => return ErrorCode::SceneNotLoaded.as_i32(),
    };
    let surface_renderer = Box::new(GsplatSurfaceRenderer {
        renderer,
        presenter,
        instances: Vec::new(),
        camera,
        camera_control,
        surface_stats: FrameStats::zero(),
        uploaded_frame: false,
        surface_sort_interval: DEFAULT_SURFACE_SORT_INTERVAL,
        surface_frames_since_sort: 0,
        surface_gpu_preproject: false,
        surface_gpu_preproject_double_buffer: false,
        surface_static_direct: false,
        surface_async_sort: false,
        surface_async_geometry: false,
        async_sorter,
        async_geometry: None,
        render_error_logged: false,
    });
    unsafe {
        *out_renderer = Box::into_raw(surface_renderer);
    }

    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_destroy(renderer: *mut GsplatSurfaceRenderer) {
    if renderer.is_null() {
        return;
    }

    unsafe {
        drop(Box::from_raw(renderer));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_resize(
    renderer: *mut GsplatSurfaceRenderer,
    width: u32,
    height: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if width == 0 || height == 0 {
        return ErrorCode::InvalidArgument.as_i32();
    }

    renderer.presenter.resize(width, height);
    let (surface_width, surface_height) = renderer.presenter.surface_size();
    if let Err(err) = renderer.renderer.set_size(surface_width, surface_height) {
        return err.code().as_i32();
    }
    renderer.camera_control = match auto_surface_camera_control(&renderer.renderer) {
        Ok(camera_control) => camera_control,
        Err(code) => return code.as_i32(),
    };
    renderer.camera =
        surface_camera_from_control(renderer.camera_control, renderer.renderer.config());
    renderer.surface_stats = FrameStats::zero();
    renderer.uploaded_frame = false;
    renderer.surface_frames_since_sort = 0;
    if let Some(async_geometry) = renderer.async_geometry.as_mut() {
        async_geometry.discard_in_flight();
    }
    renderer.render_error_logged = false;

    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_sort_interval(
    renderer: *mut GsplatSurfaceRenderer,
    interval: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    renderer.surface_sort_interval = interval.max(1);
    renderer.surface_frames_since_sort = 0;
    renderer.uploaded_frame = false;
    renderer.render_error_logged = false;
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_preproject(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let enabled = enabled != 0;
    if renderer.surface_gpu_preproject != enabled {
        renderer.surface_gpu_preproject = enabled;
        renderer.surface_frames_since_sort = 0;
        renderer.uploaded_frame = false;
        if let Some(async_geometry) = renderer.async_geometry.as_mut() {
            async_geometry.discard_in_flight();
        }
        renderer.render_error_logged = false;
    }
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_preproject_double_buffer(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let enabled = enabled != 0;
    if renderer.surface_gpu_preproject_double_buffer != enabled {
        renderer.surface_gpu_preproject_double_buffer = enabled;
        renderer.presenter.set_preproject_double_buffer(enabled);
        renderer.surface_frames_since_sort = 0;
        renderer.uploaded_frame = false;
        renderer.render_error_logged = false;
    }
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_static_direct(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let enabled = enabled != 0;
    if renderer.surface_static_direct != enabled {
        renderer.surface_static_direct = enabled;
        renderer.surface_frames_since_sort = 0;
        renderer.uploaded_frame = false;
        if let Some(async_geometry) = renderer.async_geometry.as_mut() {
            async_geometry.discard_in_flight();
        }
        renderer.render_error_logged = false;
    }
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_async_sort(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let enabled = enabled != 0;
    if renderer.surface_async_sort != enabled {
        renderer.surface_async_sort = enabled;
        renderer.async_sorter.discard_in_flight();
        renderer.surface_frames_since_sort = 0;
        renderer.uploaded_frame = false;
        renderer.render_error_logged = false;
    }
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_async_geometry(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let enabled = enabled != 0;
    if enabled && renderer.async_geometry.is_none() {
        let builder = match SurfaceInstanceBuilder::from_renderer(&renderer.renderer) {
            Ok(builder) => builder,
            Err(err) => return err.code().as_i32(),
        };
        renderer.async_geometry = Some(SurfaceGeometryWorker::new(builder));
    }
    if renderer.surface_async_geometry != enabled {
        renderer.surface_async_geometry = enabled;
        if let Some(async_geometry) = renderer.async_geometry.as_mut() {
            async_geometry.discard_in_flight();
        }
        renderer.surface_frames_since_sort = 0;
        renderer.uploaded_frame = false;
        renderer.render_error_logged = false;
    }
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_instance_buffer_count(
    renderer: *mut GsplatSurfaceRenderer,
    count: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if count == 0 {
        return ErrorCode::InvalidArgument.as_i32();
    }

    renderer.presenter.set_instance_buffer_count(count);
    renderer.uploaded_frame = false;
    renderer.render_error_logged = false;
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_frame_latency(
    renderer: *mut GsplatSurfaceRenderer,
    latency: u32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if latency == 0 {
        return ErrorCode::InvalidArgument.as_i32();
    }

    renderer.presenter.set_frame_latency(latency);
    renderer.uploaded_frame = false;
    renderer.render_error_logged = false;
    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_reset_camera(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    renderer.camera_control = match auto_surface_camera_control(&renderer.renderer) {
        Ok(camera_control) => camera_control,
        Err(code) => return code.as_i32(),
    };
    let rc = apply_surface_camera_control(renderer);
    if rc == ErrorCode::Ok.as_i32() {
        renderer.surface_frames_since_sort = 0;
    }
    rc
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_orbit(
    renderer: *mut GsplatSurfaceRenderer,
    delta_yaw_radians: f32,
    delta_pitch_radians: f32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if !delta_yaw_radians.is_finite() || !delta_pitch_radians.is_finite() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    renderer.camera_control.yaw += delta_yaw_radians;
    renderer.camera_control.pitch = (renderer.camera_control.pitch + delta_pitch_radians)
        .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
    apply_surface_camera_control(renderer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_zoom(
    renderer: *mut GsplatSurfaceRenderer,
    distance_scale: f32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return ErrorCode::InvalidArgument.as_i32();
    }

    let min_distance =
        (renderer.camera_control.radius * SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER).max(0.01);
    let max_distance =
        (renderer.camera_control.radius * SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER).max(min_distance);
    renderer.camera_control.distance =
        (renderer.camera_control.distance * distance_scale).clamp(min_distance, max_distance);
    apply_surface_camera_control(renderer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_pan(
    renderer: *mut GsplatSurfaceRenderer,
    normalized_delta_x: f32,
    normalized_delta_y: f32,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if !normalized_delta_x.is_finite() || !normalized_delta_y.is_finite() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    let config = renderer.renderer.config();
    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
    let view_height = 2.0
        * renderer.camera_control.distance
        * (renderer.camera.intrinsics.vertical_fov_radians * 0.5).tan();
    let view_width = view_height * aspect;
    let camera = surface_camera_from_control(renderer.camera_control, config);
    let (right, up, _) = camera_basis(&camera, renderer.camera_control.target);

    renderer.camera_control.target = vec3_add(
        renderer.camera_control.target,
        vec3_add(
            vec3_scale(right, -normalized_delta_x * view_width),
            vec3_scale(up, normalized_delta_y * view_height),
        ),
    );
    apply_surface_camera_control(renderer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_render_frame(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let result = if renderer.uploaded_frame && !renderer.surface_static_direct {
        renderer.presenter.render_current().map(|_| None)
    } else if renderer.surface_async_sort {
        match render_surface_frame_async_sort(renderer) {
            Ok(result) => Ok(Some(result)),
            Err(code) => return code.as_i32(),
        }
    } else if renderer.surface_async_geometry
        && !renderer.surface_gpu_preproject
        && !renderer.surface_static_direct
    {
        match render_surface_frame_async_geometry(renderer) {
            Ok(result) => Ok(Some(result)),
            Err(code) => return code.as_i32(),
        }
    } else {
        let refresh_sort = renderer.surface_sort_interval <= 1
            || renderer.surface_frames_since_sort == 0
            || renderer.surface_frames_since_sort >= renderer.surface_sort_interval;
        let stats = if renderer.surface_static_direct {
            let stats = match renderer
                .renderer
                .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, refresh_sort)
            {
                Ok(result) => result,
                Err(err) => return err.code().as_i32(),
            };
            let sorted_indices = renderer.renderer.current_sorted_indices();
            if let Err(err) = renderer.presenter.render_direct_sorted_indices(
                sorted_indices,
                &renderer.camera,
                refresh_sort,
            ) {
                return err.code().as_i32();
            }
            stats
        } else if renderer.surface_gpu_preproject {
            let stats = match renderer
                .renderer
                .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, refresh_sort)
            {
                Ok(result) => result,
                Err(err) => return err.code().as_i32(),
            };
            let sorted_indices = renderer.renderer.current_sorted_indices();
            if let Err(err) = renderer.presenter.render_sorted_indices(
                sorted_indices,
                &renderer.camera,
                refresh_sort,
            ) {
                return err.code().as_i32();
            }
            stats
        } else {
            let stats = match renderer
                .renderer
                .build_surface_instances_with_sort_refresh_into(
                    &renderer.camera,
                    &mut renderer.instances,
                    refresh_sort,
                ) {
                Ok(result) => result,
                Err(err) => return err.code().as_i32(),
            };
            if let Err(err) = renderer
                .presenter
                .render_instances(&renderer.instances, &renderer.camera)
            {
                return err.code().as_i32();
            }
            stats
        };
        Ok(Some((stats, refresh_sort)))
    };

    match result {
        Ok(stats) => {
            if let Some((stats, refresh_sort)) = stats {
                renderer.surface_stats = stats;
                renderer.surface_frames_since_sort = if refresh_sort {
                    1
                } else {
                    renderer.surface_frames_since_sort.saturating_add(1)
                };
                renderer.uploaded_frame = refresh_sort;
            } else {
                renderer.uploaded_frame = true;
            }
            renderer.render_error_logged = false;
            ErrorCode::Ok.as_i32()
        }
        Err(err) => {
            if !renderer.render_error_logged {
                log_surface_error(&format!(
                    "gsplat_surface_renderer_render_frame failed: {err:?}"
                ));
                renderer.render_error_logged = true;
            }
            err.code().as_i32()
        }
    }
}

fn render_surface_frame_async_sort(
    renderer: &mut GsplatSurfaceRenderer,
) -> Result<(FrameStats, bool), ErrorCode> {
    let mut refresh_order_upload = false;
    let mut async_sort_timing = None;
    if let Some(result) = renderer.async_sorter.poll_result() {
        let result = result?;
        renderer
            .renderer
            .replace_surface_sorted_indices(result.indices)
            .map_err(|err| err.code())?;
        renderer.surface_frames_since_sort = 0;
        refresh_order_upload = true;
        async_sort_timing = Some((result.preprocess_ms, result.sort_ms));
    }

    if renderer.renderer.current_sorted_indices().is_empty() {
        return render_surface_frame_sync_sort(renderer, true);
    }

    let interval = renderer.surface_sort_interval.max(1);
    let should_schedule = !renderer.async_sorter.is_in_flight()
        && renderer.surface_frames_since_sort.saturating_add(1) >= interval;

    let mut stats = if renderer.surface_static_direct {
        let stats = renderer
            .renderer
            .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, false)
            .map_err(|err| err.code())?;
        let sorted_indices = renderer.renderer.current_sorted_indices();
        renderer
            .presenter
            .render_direct_sorted_indices(sorted_indices, &renderer.camera, refresh_order_upload)
            .map_err(|err| err.code())?;
        stats
    } else if renderer.surface_gpu_preproject {
        let stats = renderer
            .renderer
            .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, false)
            .map_err(|err| err.code())?;
        let sorted_indices = renderer.renderer.current_sorted_indices();
        renderer
            .presenter
            .render_sorted_indices(sorted_indices, &renderer.camera, refresh_order_upload)
            .map_err(|err| err.code())?;
        stats
    } else {
        let stats = renderer
            .renderer
            .build_surface_instances_with_sort_refresh_into(
                &renderer.camera,
                &mut renderer.instances,
                false,
            )
            .map_err(|err| err.code())?;
        renderer
            .presenter
            .render_instances(&renderer.instances, &renderer.camera)
            .map_err(|err| err.code())?;
        stats
    };

    if let Some((preprocess_ms, sort_ms)) = async_sort_timing {
        stats.preprocess_ms = preprocess_ms;
        stats.sort_ms = sort_ms;
    }

    if should_schedule {
        renderer.async_sorter.start(renderer.camera);
    }

    Ok((stats, !renderer.async_sorter.is_in_flight()))
}

fn render_surface_frame_async_geometry(
    renderer: &mut GsplatSurfaceRenderer,
) -> Result<(FrameStats, bool), ErrorCode> {
    let mut completed_build_ms = None;
    let async_geometry = renderer
        .async_geometry
        .as_mut()
        .ok_or(ErrorCode::Internal)?;
    if let Some(result) = async_geometry.poll_result() {
        let result = result?;
        renderer.instances = result.instances;
        completed_build_ms = Some(result.build_ms);
    }

    let refresh_sort = renderer.surface_sort_interval <= 1
        || renderer.surface_frames_since_sort == 0
        || renderer.surface_frames_since_sort >= renderer.surface_sort_interval;
    let mut stats = renderer
        .renderer
        .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, refresh_sort)
        .map_err(|err| err.code())?;

    if renderer.instances.is_empty() {
        stats = renderer
            .renderer
            .build_surface_instances_with_sort_refresh_into(
                &renderer.camera,
                &mut renderer.instances,
                false,
            )
            .map_err(|err| err.code())?;
        completed_build_ms = None;
    }

    renderer
        .presenter
        .render_instances(&renderer.instances, &renderer.camera)
        .map_err(|err| err.code())?;

    if let Some(build_ms) = completed_build_ms {
        stats.raster_ms = build_ms;
        stats.frame_ms = stats.preprocess_ms + stats.sort_ms + build_ms;
        stats.drawn_count = renderer.instances.len() as u32;
    }

    let async_geometry = renderer
        .async_geometry
        .as_mut()
        .ok_or(ErrorCode::Internal)?;
    if !async_geometry.is_in_flight() {
        let indices = renderer.renderer.current_sorted_indices().to_vec();
        async_geometry.start(renderer.camera, renderer.renderer.config(), indices);
    }

    Ok((stats, refresh_sort))
}

fn render_surface_frame_sync_sort(
    renderer: &mut GsplatSurfaceRenderer,
    refresh_sort: bool,
) -> Result<(FrameStats, bool), ErrorCode> {
    let stats = if renderer.surface_static_direct {
        let stats = renderer
            .renderer
            .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, refresh_sort)
            .map_err(|err| err.code())?;
        let sorted_indices = renderer.renderer.current_sorted_indices();
        renderer
            .presenter
            .render_direct_sorted_indices(sorted_indices, &renderer.camera, refresh_sort)
            .map_err(|err| err.code())?;
        stats
    } else if renderer.surface_gpu_preproject {
        let stats = renderer
            .renderer
            .build_surface_sorted_indices_with_sort_refresh(&renderer.camera, refresh_sort)
            .map_err(|err| err.code())?;
        let sorted_indices = renderer.renderer.current_sorted_indices();
        renderer
            .presenter
            .render_sorted_indices(sorted_indices, &renderer.camera, refresh_sort)
            .map_err(|err| err.code())?;
        stats
    } else {
        let stats = renderer
            .renderer
            .build_surface_instances_with_sort_refresh_into(
                &renderer.camera,
                &mut renderer.instances,
                refresh_sort,
            )
            .map_err(|err| err.code())?;
        renderer
            .presenter
            .render_instances(&renderer.instances, &renderer.camera)
            .map_err(|err| err.code())?;
        stats
    };
    Ok((stats, refresh_sort))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_stats(
    renderer: *const GsplatSurfaceRenderer,
    out_stats: *mut GsplatStats,
) -> i32 {
    let renderer = match unsafe { renderer.as_ref() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    if out_stats.is_null() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    unsafe {
        *out_stats = renderer.surface_stats.into();
    }

    ErrorCode::Ok.as_i32()
}

fn sort_positions_for_camera(
    positions: &[Vec3f],
    camera: Camera,
) -> Result<AsyncSortResult, ErrorCode> {
    camera.validate()?;

    let preprocess_start = Instant::now();
    let camera_inv_q = async_quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = async_quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;
    let mut depth_keys = Vec::with_capacity(positions.len());
    let mut indices = Vec::with_capacity(positions.len());
    for (idx, &position) in positions.iter().enumerate() {
        let p = vec3_sub(position, camera_position);
        let depth_z = depth_row[0] * p.x + depth_row[1] * p.y + depth_row[2] * p.z;
        if depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane {
            indices.push(idx as u32);
            depth_keys.push(async_depth_to_key(depth_z));
        }
    }
    let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

    let sort_start = Instant::now();
    CpuSortBackend::default()
        .sort_values_by_keys(&depth_keys, &mut indices)
        .map_err(|_| ErrorCode::Internal)?;
    let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

    Ok(AsyncSortResult {
        indices,
        preprocess_ms,
        sort_ms,
    })
}

fn async_depth_to_key(depth_z: f32) -> u32 {
    depth_z.max(0.0).to_bits()
}

fn async_quat_inverse(q: [f32; 4]) -> [f32; 4] {
    let q = async_quat_normalize(q);
    [-q[0], -q[1], -q[2], q[3]]
}

fn async_quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let norm2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if norm2 <= 0.0 || !norm2.is_finite() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 / norm2.sqrt();
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

fn async_quat_to_mat3(q: [f32; 4]) -> [[f32; 3]; 3] {
    let q = async_quat_normalize(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;

    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
        [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
        [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
    ]
}

fn auto_camera(renderer: &Renderer) -> Result<Camera, ErrorCode> {
    let camera_control = auto_surface_camera_control(renderer)?;
    Ok(surface_camera_from_control(
        camera_control,
        renderer.config(),
    ))
}

fn auto_surface_camera_control(renderer: &Renderer) -> Result<SurfaceCameraControl, ErrorCode> {
    let Some(scene) = renderer.scene() else {
        return Err(ErrorCode::SceneNotLoaded);
    };
    let Some((min, max)) = scene_bounds(scene) else {
        return Err(ErrorCode::InvalidArgument);
    };

    let config = renderer.config();
    let center = Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    );
    let extent = Vec3f::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let half_x = (extent.x * 0.5).max(1e-3);
    let half_y = (extent.y * 0.5).max(1e-3);
    let half_z = (extent.z * 0.5).max(1e-3);

    let aspect = (config.width as f32) / (config.height as f32);
    let vfov = Camera::default().intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    let dist = (dist_y.max(dist_x) + half_z) * 1.2;
    let radius = half_x.max(half_y).max(half_z);

    Ok(SurfaceCameraControl {
        target: center,
        radius,
        yaw: 0.0,
        pitch: 0.0,
        distance: dist,
    })
}

fn apply_surface_camera_control(renderer: &mut GsplatSurfaceRenderer) -> i32 {
    let camera = surface_camera_from_control(renderer.camera_control, renderer.renderer.config());
    if camera.validate().is_err() {
        return ErrorCode::InvalidArgument.as_i32();
    }

    renderer.camera = camera;
    renderer.uploaded_frame = false;
    renderer.render_error_logged = false;
    ErrorCode::Ok.as_i32()
}

fn surface_camera_from_control(control: SurfaceCameraControl, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    let pitch = control
        .pitch
        .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
    let cos_pitch = pitch.cos();
    let offset = Vec3f::new(
        control.yaw.sin() * cos_pitch * control.distance,
        pitch.sin() * control.distance,
        -control.yaw.cos() * cos_pitch * control.distance,
    );
    camera.pose.position = vec3_add(control.target, offset);
    camera.pose.rotation_xyzw = camera_rotation_looking_at(camera.pose.position, control.target);

    let radius = control.radius.max(1.0e-3);
    camera.intrinsics.near_plane = (control.distance - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (control.distance + radius * 8.0).max(100.0);

    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
    if aspect < 0.6 {
        camera.intrinsics.vertical_fov_radians = 65.0_f32.to_radians();
    }

    camera
}

fn camera_rotation_looking_at(position: Vec3f, target: Vec3f) -> [f32; 4] {
    let (_, _, forward) = camera_basis_from_position(position, target);
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    quat_from_camera_basis(right, up, forward)
}

fn camera_basis(camera: &Camera, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    camera_basis_from_position(camera.pose.position, target)
}

fn camera_basis_from_position(position: Vec3f, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let forward = vec3_normalize(vec3_sub(target, position)).unwrap_or(Vec3f::new(0.0, 0.0, 1.0));
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    (right, up, forward)
}

fn quat_from_camera_basis(right: Vec3f, up: Vec3f, forward: Vec3f) -> [f32; 4] {
    let m00 = right.x;
    let m01 = up.x;
    let m02 = forward.x;
    let m10 = right.y;
    let m11 = up.y;
    let m12 = forward.y;
    let m20 = right.z;
    let m21 = up.z;
    let m22 = forward.z;
    let trace = m00 + m11 + m22;

    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        ((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s)
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        (0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s)
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        ((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s)
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        ((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s)
    };

    let norm = (x * x + y * y + z * z + w * w).sqrt().max(1.0e-6);
    [x / norm, y / norm, z / norm, w / norm]
}

fn vec3_add(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn vec3_sub(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn vec3_scale(v: Vec3f, scale: f32) -> Vec3f {
    Vec3f::new(v.x * scale, v.y * scale, v.z * scale)
}

fn vec3_cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn vec3_normalize(v: Vec3f) -> Option<Vec3f> {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if !len.is_finite() || len <= 1.0e-6 {
        return None;
    }

    Some(vec3_scale(v, 1.0 / len))
}

fn scene_bounds(scene: &gsplat_core::SceneBuffers) -> Option<(Vec3f, Vec3f)> {
    if scene.positions.is_empty() {
        return None;
    }
    let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in &scene.positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }
    Some((min, max))
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use gsplat_core::ErrorCode;

    use super::{
        GsplatCamera, GsplatConfig, GsplatContext, SurfaceCameraControl,
        camera_rotation_looking_at, gsplat_camera_default, gsplat_config_default,
        gsplat_context_create, gsplat_context_destroy, gsplat_context_set_camera,
        gsplat_error_message, surface_camera_from_control,
    };

    #[test]
    fn default_ffi_values_match_release_contract() {
        let config = gsplat_config_default();
        assert_eq!(config.width, 1280);
        assert_eq!(config.height, 720);
        assert_eq!(config.mode, 0);

        let camera = gsplat_camera_default();
        assert_eq!(camera.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(camera.near_plane, 0.01);
        assert_eq!(camera.far_plane, 1000.0);
    }

    #[test]
    fn error_message_describes_known_and_unknown_codes() {
        let invalid = unsafe { std::ffi::CStr::from_ptr(gsplat_error_message(1)) };
        assert_eq!(invalid.to_str().unwrap(), "invalid argument");

        let unknown = unsafe { std::ffi::CStr::from_ptr(gsplat_error_message(-1)) };
        assert_eq!(unknown.to_str().unwrap(), "unknown error");
    }

    #[test]
    fn surface_camera_control_matches_default_view_direction() {
        let control = SurfaceCameraControl {
            target: gsplat_core::Vec3f::new(1.0, 2.0, 3.0),
            radius: 2.0,
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
        };

        let camera = surface_camera_from_control(control, gsplat_core::RendererConfig::default());

        assert_eq!(
            camera.pose.position,
            gsplat_core::Vec3f::new(1.0, 2.0, -7.0)
        );
        assert_eq!(camera.pose.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn looking_at_rotation_is_normalized_for_orbited_camera() {
        let q = camera_rotation_looking_at(
            gsplat_core::Vec3f::new(2.0, 1.0, -4.0),
            gsplat_core::Vec3f::new(0.0, 0.0, 0.0),
        );
        let norm2 = q.iter().map(|v| v * v).sum::<f32>();

        assert!((norm2 - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn create_and_destroy_context() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();

        let rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn context_create_rejects_non_release_render_mode() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let config = GsplatConfig {
            mode: 1,
            ..GsplatConfig::default()
        };

        let rc = unsafe { gsplat_context_create(config, &mut ctx) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        assert!(ctx.is_null());
    }

    #[test]
    fn context_set_camera_rejects_invalid_intrinsics() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let create_rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(create_rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        let camera = GsplatCamera {
            near_plane: 10.0,
            far_plane: 1.0,
            ..GsplatCamera::default()
        };
        let rc = unsafe { gsplat_context_set_camera(ctx, camera) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        unsafe { gsplat_context_destroy(ctx) };
    }
}
