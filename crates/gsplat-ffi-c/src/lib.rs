//! Stable C ABI surface for mobile wrappers.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Display;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::NonNull;

use gsplat_core::{
    Camera, CameraIntrinsics, CameraPose, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR,
    GSPLAT_API_VERSION_MINOR, RenderMode, RendererConfig, Vec3f,
};
use gsplat_io_ply::load_ply;
#[cfg(target_os = "android")]
use gsplat_render_wgpu::SurfaceOrderBackend;
use gsplat_render_wgpu::{
    GeometryPath, Renderer, SurfaceAdaptiveState, SurfaceFrameOutput, SurfaceOrderBackendUsed,
    SurfacePresenter, SurfaceRenderSession,
};

const SURFACE_CAMERA_MAX_PITCH: f32 = 1.45;
const SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER: f32 = 0.2;
const SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER: f32 = 20.0;

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GsplatSurfaceSortStats {
    pub camera_revision: u64,
    pub applied_order_revision: u64,
    pub scheduled_revision: u64,
    pub completed_revision: u64,
    pub presented_order_revision_lag: u32,
    pub observed_result_revision_lag: u32,
    pub flags: u32,
}

impl From<SurfaceFrameOutput> for GsplatSurfaceSortStats {
    fn from(output: SurfaceFrameOutput) -> Self {
        let mut flags = 0_u32;
        flags |= u32::from(output.sort_refreshed);
        flags |= u32::from(output.order_uploaded) << 1;
        flags |= u32::from(output.async_sort_scheduled) << 2;
        flags |= u32::from(output.async_sort_scheduled_revision.is_some()) << 3;
        flags |= u32::from(output.async_sort_completed_revision.is_some()) << 4;
        flags |= u32::from(output.async_sort_result_applied) << 5;
        flags |= u32::from(output.stale_async_sort_dropped) << 6;
        flags |= u32::from(output.sync_sort_fallback) << 7;
        flags |= u32::from(output.async_sort_revision_lag.is_some()) << 8;
        flags |= match output.order_backend {
            SurfaceOrderBackendUsed::Cpu => 0,
            SurfaceOrderBackendUsed::Gpu => 1,
        } << 9;
        flags |= u32::from(output.gpu_sort_fallback) << 11;
        flags |= match output.adaptive_state {
            SurfaceAdaptiveState::Disabled => 0,
            SurfaceAdaptiveState::CpuLearning => 1,
            SurfaceAdaptiveState::CpuStable => 2,
            SurfaceAdaptiveState::GpuProbe => 3,
            SurfaceAdaptiveState::GpuStable => 4,
            SurfaceAdaptiveState::CpuProbe => 5,
            SurfaceAdaptiveState::Cooldown => 6,
        } << 12;
        Self {
            camera_revision: output.camera_revision,
            applied_order_revision: output.applied_order_revision,
            scheduled_revision: output.async_sort_scheduled_revision.unwrap_or(0),
            completed_revision: output.async_sort_completed_revision.unwrap_or(0),
            presented_order_revision_lag: output.presented_order_revision_lag,
            observed_result_revision_lag: output.async_sort_revision_lag.unwrap_or(0),
            flags,
        }
    }
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
    session: SurfaceRenderSession,
    camera_control: SurfaceCameraControl,
    render_error_logged: bool,
    last_sort_stats: GsplatSurfaceSortStats,
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

thread_local! {
    static LAST_ERROR_MESSAGE: RefCell<CString> =
        RefCell::new(CString::new("ok").expect("static ok string contains no nul"));
}

fn make_cstring(message: impl Into<String>) -> CString {
    let bytes: Vec<u8> = message
        .into()
        .into_bytes()
        .into_iter()
        .filter(|byte| *byte != 0)
        .collect();
    CString::new(bytes).unwrap_or_else(|_| CString::new("internal error").unwrap())
}

fn set_last_error_message(message: impl Into<String>) {
    LAST_ERROR_MESSAGE.with(|cell| {
        *cell.borrow_mut() = make_cstring(message);
    });
}

fn ffi_ok() -> i32 {
    set_last_error_message("ok");
    ErrorCode::Ok.as_i32()
}

fn ffi_error(code: ErrorCode, message: impl Into<String>) -> i32 {
    set_last_error_message(message);
    code.as_i32()
}

fn ffi_error_display(code: ErrorCode, operation: &str, error: impl Display) -> i32 {
    ffi_error(code, format!("{operation}: {error}"))
}

fn record_ffi_panic(operation: &str) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        set_last_error_message(format!("{operation}: panic caught at C ABI boundary"));
    }));
}

fn ffi_catch_i32(operation: &str, body: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            record_ffi_panic(operation);
            ErrorCode::Internal.as_i32()
        }
    }
}

fn ffi_catch_void(operation: &str, body: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(body)).is_err() {
        record_ffi_panic(operation);
    }
}

fn ffi_catch_value<T>(operation: &str, fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            record_ffi_panic(operation);
            fallback
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_version_major() -> u32 {
    ffi_catch_value("gsplat_version_major", 0, || GSPLAT_API_VERSION_MAJOR)
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_version_minor() -> u32 {
    ffi_catch_value("gsplat_version_minor", 0, || GSPLAT_API_VERSION_MINOR)
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_error_message(code: i32) -> *const c_char {
    ffi_catch_value(
        "gsplat_error_message",
        static_cstr(b"internal error\0"),
        || match code {
            0 => static_cstr(b"ok\0"),
            1 => static_cstr(b"invalid argument\0"),
            2 => static_cstr(b"not found\0"),
            3 => static_cstr(b"parse failed\0"),
            4 => static_cstr(b"unsupported\0"),
            5 => static_cstr(b"scene not loaded\0"),
            100 => static_cstr(b"internal error\0"),
            _ => static_cstr(b"unknown error\0"),
        },
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_last_error_message() -> *const c_char {
    ffi_catch_value(
        "gsplat_last_error_message",
        static_cstr(b"internal error\0"),
        || LAST_ERROR_MESSAGE.with(|cell| cell.borrow().as_ptr()),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_config_default() -> GsplatConfig {
    ffi_catch_value(
        "gsplat_config_default",
        GsplatConfig {
            width: 0,
            height: 0,
            mode: RenderMode::SortedAlpha as u32,
        },
        GsplatConfig::default,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn gsplat_camera_default() -> GsplatCamera {
    ffi_catch_value(
        "gsplat_camera_default",
        GsplatCamera {
            position: [0.0; 3],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            vertical_fov_radians: 0.0,
            near_plane: 0.0,
            far_plane: 0.0,
        },
        GsplatCamera::default,
    )
}

/// Create a renderer context.
///
/// # Safety
///
/// `out_ctx` must be valid for one pointer write. On success, the returned
/// handle must be released exactly once with `gsplat_context_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_create(
    config: GsplatConfig,
    out_ctx: *mut *mut GsplatContext,
) -> i32 {
    ffi_catch_i32("gsplat_context_create", || {
        if out_ctx.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_create: out_ctx is null",
            );
        }

        unsafe {
            *out_ctx = std::ptr::null_mut();
        }

        if config.mode != RenderMode::SortedAlpha as u32 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_create: unsupported render mode",
            );
        }

        let renderer_config = RendererConfig {
            width: config.width,
            height: config.height,
            mode: RenderMode::SortedAlpha,
        };

        let renderer = match Renderer::with_config(renderer_config) {
            Ok(renderer) => renderer,
            Err(err) => return ffi_error_display(err.code(), "gsplat_context_create", err),
        };

        let context = Box::new(GsplatContext {
            renderer,
            camera: Camera::default(),
        });

        unsafe {
            *out_ctx = Box::into_raw(context);
        }

        ffi_ok()
    })
}

/// Destroy a renderer context.
///
/// # Safety
///
/// `ctx` may be null. Non-null values must be handles returned by
/// `gsplat_context_create` that have not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_destroy(ctx: *mut GsplatContext) {
    ffi_catch_void("gsplat_context_destroy", || {
        if ctx.is_null() {
            return;
        }

        unsafe {
            drop(Box::from_raw(ctx));
        }
    });
}

/// Replace the current camera for a context.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_set_camera(
    ctx: *mut GsplatContext,
    camera: GsplatCamera,
) -> i32 {
    ffi_catch_i32("gsplat_context_set_camera", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_set_camera: ctx is null",
                );
            }
        };

        let camera: Camera = camera.into();
        if camera.validate().is_err() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_set_camera: invalid camera",
            );
        }

        ctx.camera = camera;
        ffi_ok()
    })
}

/// Set the context camera to an automatically framed view of the loaded scene.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_set_auto_camera(ctx: *mut GsplatContext) -> i32 {
    ffi_catch_i32("gsplat_context_set_auto_camera", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_set_auto_camera: ctx is null",
                );
            }
        };

        match auto_camera(&ctx.renderer) {
            Ok(camera) => {
                ctx.camera = camera;
                ffi_ok()
            }
            Err(code) => ffi_error(code, "gsplat_context_set_auto_camera: scene is not loaded"),
        }
    })
}

/// Load a scene from a filesystem path.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
/// `path` must be a non-null, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_load_scene_path(
    ctx: *mut GsplatContext,
    path: *const c_char,
) -> i32 {
    ffi_catch_i32("gsplat_context_load_scene_path", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_load_scene_path: ctx is null",
                );
            }
        };

        if path.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_load_scene_path: path is null",
            );
        }

        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(path) => path,
            Err(_) => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_load_scene_path: path is not valid UTF-8",
                );
            }
        };

        let loaded = match load_ply(Path::new(path_str)) {
            Ok(result) => result,
            Err(err) => {
                return ffi_error_display(err.code(), "gsplat_context_load_scene_path", err);
            }
        };

        match ctx.renderer.load_scene(loaded.scene) {
            Ok(()) => ffi_ok(),
            Err(err) => ffi_error_display(err.code(), "gsplat_context_load_scene_path", err),
        }
    })
}

/// Render one offscreen frame for a context.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_render_frame(ctx: *mut GsplatContext) -> i32 {
    ffi_catch_i32("gsplat_context_render_frame", || {
        let ctx = match unsafe { ctx.as_mut() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_render_frame: ctx is null",
                );
            }
        };

        match ctx.renderer.render_frame(&ctx.camera) {
            Ok(_) => ffi_ok(),
            Err(err) => ffi_error_display(err.code(), "gsplat_context_render_frame", err),
        }
    })
}

/// Copy the last frame stats for a context.
///
/// # Safety
///
/// `ctx` must be null or a live handle returned by `gsplat_context_create`.
/// `out_stats` must be valid for one `GsplatStats` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_context_get_stats(
    ctx: *const GsplatContext,
    out_stats: *mut GsplatStats,
) -> i32 {
    ffi_catch_i32("gsplat_context_get_stats", || {
        let ctx = match unsafe { ctx.as_ref() } {
            Some(ctx) => ctx,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_context_get_stats: ctx is null",
                );
            }
        };

        if out_stats.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_context_get_stats: out_stats is null",
            );
        }

        unsafe {
            *out_stats = ctx.renderer.last_stats().into();
        }

        ffi_ok()
    })
}

/// Create an Android Surface renderer from an `ANativeWindow`.
///
/// # Safety
///
/// `native_window` must be a valid `ANativeWindow` for the lifetime required
/// by the created Surface renderer. `path` must be a non-null, NUL-terminated
/// C string. `out_renderer` must be valid for one pointer write. On success,
/// the returned handle must be destroyed with `gsplat_surface_renderer_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_android(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_android_surface_renderer(
            native_window,
            path,
            width,
            height,
            0,
            out_renderer,
            "gsplat_surface_renderer_create_android",
        )
    }
}

/// Create an Android Surface renderer with a preselected experimental geometry path.
///
/// # Safety
///
/// The pointer requirements match [`gsplat_surface_renderer_create_android`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_android_with_geometry_path(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_android_surface_renderer(
            native_window,
            path,
            width,
            height,
            geometry_path,
            out_renderer,
            "gsplat_surface_renderer_create_android_with_geometry_path",
        )
    }
}

unsafe fn create_android_surface_renderer(
    native_window: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
    operation: &'static str,
) -> i32 {
    ffi_catch_i32(operation, || {
        if native_window.is_null() || path.is_null() || out_renderer.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                format!("{operation}: native_window, path, or out_renderer is null"),
            );
        }

        let geometry_path = match geometry_path_from_ffi(geometry_path) {
            Some(path) => path,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: unsupported geometry path"),
                );
            }
        };

        unsafe {
            *out_renderer = std::ptr::null_mut();
        }

        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(path) => path,
            Err(_) => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: path is not valid UTF-8"),
                );
            }
        };

        let mut renderer = match Renderer::with_config_for_surface(RendererConfig {
            width,
            height,
            mode: RenderMode::SortedAlpha,
        }) {
            Ok(renderer) => renderer,
            Err(err) => {
                return ffi_error_display(err.code(), operation, err);
            }
        };
        renderer.set_geometry_path(geometry_path);

        let loaded = match load_ply(Path::new(path_str)) {
            Ok(result) => result,
            Err(err) => {
                return ffi_error_display(err.code(), operation, err);
            }
        };
        if let Err(err) = renderer.load_scene(loaded.scene) {
            return ffi_error_display(err.code(), operation, err);
        };

        let window = match NonNull::new(native_window) {
            Some(window) => window,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: native_window is null"),
                );
            }
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
    })
}

/// Create a UIKit Surface renderer from a view backed by `CAMetalLayer`.
///
/// # Safety
///
/// `ui_view` must be a valid UIKit view backed by `CAMetalLayer`.
/// `ui_view_controller` may be null. `path` must be a non-null,
/// NUL-terminated C string. `out_renderer` must be valid for one pointer
/// write. On success, the returned handle must be destroyed with
/// `gsplat_surface_renderer_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_uikit(
    ui_view: *mut c_void,
    ui_view_controller: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_uikit_surface_renderer(
            (ui_view, ui_view_controller),
            path,
            width,
            height,
            0,
            out_renderer,
            "gsplat_surface_renderer_create_uikit",
        )
    }
}

/// Create a UIKit Surface renderer with a preselected experimental geometry path.
///
/// # Safety
///
/// The pointer requirements match [`gsplat_surface_renderer_create_uikit`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_create_uikit_with_geometry_path(
    ui_view: *mut c_void,
    ui_view_controller: *mut c_void,
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
) -> i32 {
    unsafe {
        create_uikit_surface_renderer(
            (ui_view, ui_view_controller),
            path,
            width,
            height,
            geometry_path,
            out_renderer,
            "gsplat_surface_renderer_create_uikit_with_geometry_path",
        )
    }
}

unsafe fn create_uikit_surface_renderer(
    ui_target: (*mut c_void, *mut c_void),
    path: *const c_char,
    width: u32,
    height: u32,
    geometry_path: u32,
    out_renderer: *mut *mut GsplatSurfaceRenderer,
    operation: &'static str,
) -> i32 {
    ffi_catch_i32(operation, || {
        let (ui_view, ui_view_controller) = ui_target;
        if ui_view.is_null() || path.is_null() || out_renderer.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                format!("{operation}: ui_view, path, or out_renderer is null"),
            );
        }

        let geometry_path = match geometry_path_from_ffi(geometry_path) {
            Some(path) => path,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: unsupported geometry path"),
                );
            }
        };

        unsafe {
            *out_renderer = std::ptr::null_mut();
        }

        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(path) => path,
            Err(_) => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: path is not valid UTF-8"),
                );
            }
        };

        let mut renderer = match Renderer::with_config_for_surface(RendererConfig {
            width,
            height,
            mode: RenderMode::SortedAlpha,
        }) {
            Ok(renderer) => renderer,
            Err(err) => {
                return ffi_error_display(err.code(), operation, err);
            }
        };
        renderer.set_geometry_path(geometry_path);

        let loaded = match load_ply(Path::new(path_str)) {
            Ok(result) => result,
            Err(err) => {
                return ffi_error_display(err.code(), operation, err);
            }
        };
        if let Err(err) = renderer.load_scene(loaded.scene) {
            return ffi_error_display(err.code(), operation, err);
        };

        let view = match NonNull::new(ui_view) {
            Some(view) => view,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    format!("{operation}: ui_view is null"),
                );
            }
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
    })
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
            return ffi_error_display(err.code(), "SurfacePresenter::from_raw_handles", err);
        }
    };

    let (surface_width, surface_height) = presenter.surface_size();
    if let Err(err) = renderer.set_size(surface_width, surface_height) {
        return ffi_error_display(err.code(), "gsplat_surface_renderer_set_size", err);
    }
    let camera_control = match auto_surface_camera_control(&renderer) {
        Ok(camera_control) => camera_control,
        Err(code) => {
            return ffi_error(
                code,
                "gsplat_surface_renderer_create: failed to create automatic camera",
            );
        }
    };
    let camera = surface_camera_from_control(camera_control, renderer.config());
    let session = match SurfaceRenderSession::new(renderer, presenter, camera) {
        Ok(session) => session,
        Err(err) => {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_create", err);
        }
    };
    let surface_renderer = Box::new(GsplatSurfaceRenderer {
        session,
        camera_control,
        render_error_logged: false,
        last_sort_stats: GsplatSurfaceSortStats::default(),
    });
    unsafe {
        *out_renderer = Box::into_raw(surface_renderer);
    }

    ffi_ok()
}

/// Destroy a Surface renderer.
///
/// # Safety
///
/// `renderer` may be null. Non-null values must be handles returned by a
/// Surface renderer create function that have not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_destroy(renderer: *mut GsplatSurfaceRenderer) {
    ffi_catch_void("gsplat_surface_renderer_destroy", || {
        if renderer.is_null() {
            return;
        }

        unsafe {
            drop(Box::from_raw(renderer));
        }
    });
}

/// Resize a Surface renderer.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_resize(
    renderer: *mut GsplatSurfaceRenderer,
    width: u32,
    height: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_resize", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_resize: renderer is null",
                );
            }
        };

        if width == 0 || height == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_resize: width and height must be positive",
            );
        }

        if let Err(err) = renderer.session.resize(width, height) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_resize", err);
        }
        renderer.camera_control = match auto_surface_camera_control(renderer.session.renderer()) {
            Ok(camera_control) => camera_control,
            Err(code) => {
                return ffi_error(
                    code,
                    "gsplat_surface_renderer_resize: failed to reset automatic camera",
                );
            }
        };
        let camera = surface_camera_from_control(
            renderer.camera_control,
            renderer.session.renderer().config(),
        );
        if let Err(err) = renderer.session.set_camera(camera) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_resize", err);
        }
        renderer.render_error_logged = false;

        ffi_ok()
    })
}

/// Set the Surface renderer sort interval in frames.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_sort_interval(
    renderer: *mut GsplatSurfaceRenderer,
    interval: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_sort_interval", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_sort_interval: renderer is null",
                );
            }
        };

        if interval == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_sort_interval: interval must be positive",
            );
        }

        if let Err(err) = renderer.session.set_sort_interval(interval) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_set_sort_interval", err);
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Set the Surface renderer geometry path.
///
/// `path` must be `GSPLAT_GEOMETRY_PATH_DIRECT` (0),
/// `GSPLAT_GEOMETRY_PATH_PACKED_ATLAS` (1), or
/// `GSPLAT_GEOMETRY_PATH_PAGED_ACTIVE_ATLAS` (2). This is an experimental A/B
/// benchmark knob; the default remains the direct sorted-index path.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_geometry_path(
    renderer: *mut GsplatSurfaceRenderer,
    path: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_geometry_path", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_geometry_path: renderer is null",
                );
            }
        };

        let geometry_path = match geometry_path_from_ffi(path) {
            Some(path) => path,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_geometry_path: unsupported path",
                );
            }
        };

        if let Err(err) = renderer.session.set_geometry_path(geometry_path) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_set_geometry_path", err);
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

fn geometry_path_from_ffi(path: u32) -> Option<GeometryPath> {
    match path {
        0 => Some(GeometryPath::SortedIndexDirect),
        1 => Some(GeometryPath::PackedAtlas),
        2 => Some(GeometryPath::PagedActiveAtlas),
        _ => None,
    }
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// CPU-sorted-index rendering is always used; `gsplat_surface_renderer_set_geometry_path`
/// is the only supported geometry A/B knob.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_preproject(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_gpu_preproject", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_gpu_preproject: renderer is null",
                );
            }
        };

        let _ = enabled;
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_gpu_preproject_double_buffer(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32(
        "gsplat_surface_renderer_set_gpu_preproject_double_buffer",
        || {
            let renderer = match unsafe { renderer.as_mut() } {
                Some(renderer) => renderer,
                None => {
                    return ffi_error(
                        ErrorCode::InvalidArgument,
                        "gsplat_surface_renderer_set_gpu_preproject_double_buffer: renderer is null",
                    );
                }
            };

            let _ = enabled;
            renderer.render_error_logged = false;
            ffi_ok()
        },
    )
}

/// Enable or disable experimental async sorting.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_async_sort(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_async_sort", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_async_sort: renderer is null",
                );
            }
        };

        if let Err(err) = renderer.session.set_async_sort_enabled(enabled != 0) {
            return ffi_error_display(err.code(), "gsplat_surface_renderer_set_async_sort", err);
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Android sample-only forced backend knob used to collect paired benchmark
/// evidence without widening the published v0.1 header.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_android_benchmark_set_order_backend(
    renderer: *mut GsplatSurfaceRenderer,
    backend: u32,
) -> i32 {
    ffi_catch_i32("gsplat_android_benchmark_set_order_backend", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_android_benchmark_set_order_backend: renderer is null",
                );
            }
        };
        let backend = match backend {
            0 => SurfaceOrderBackend::Cpu,
            1 => SurfaceOrderBackend::Gpu,
            2 => SurfaceOrderBackend::Adaptive,
            _ => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_android_benchmark_set_order_backend: unsupported backend",
                );
            }
        };
        if let Err(err) = renderer.session.set_order_backend(backend) {
            return ffi_error_display(
                err.code(),
                "gsplat_android_benchmark_set_order_backend",
                err,
            );
        }
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_async_geometry(
    renderer: *mut GsplatSurfaceRenderer,
    enabled: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_async_geometry", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_async_geometry: renderer is null",
                );
            }
        };

        let _ = enabled;
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Compatibility no-op retained for the v0.1 ABI.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_instance_buffer_count(
    renderer: *mut GsplatSurfaceRenderer,
    count: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_instance_buffer_count", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_instance_buffer_count: renderer is null",
                );
            }
        };

        if count == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_instance_buffer_count: count must be positive",
            );
        }

        let _ = count;
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Set the preferred Surface frame latency.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_set_frame_latency(
    renderer: *mut GsplatSurfaceRenderer,
    latency: u32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_set_frame_latency", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_set_frame_latency: renderer is null",
                );
            }
        };

        if latency == 0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_set_frame_latency: latency must be positive",
            );
        }

        renderer.session.set_frame_latency(latency);
        renderer.render_error_logged = false;
        ffi_ok()
    })
}

/// Reset the Surface camera to the automatic scene framing.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_reset_camera(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_reset_camera", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_reset_camera: renderer is null",
                );
            }
        };

        renderer.camera_control = match auto_surface_camera_control(renderer.session.renderer()) {
            Ok(camera_control) => camera_control,
            Err(code) => {
                return ffi_error(
                    code,
                    "gsplat_surface_renderer_reset_camera: failed to create automatic camera",
                );
            }
        };
        renderer.session.force_sort_refresh();
        apply_surface_camera_control(renderer)
    })
}

/// Orbit the Surface camera by yaw and pitch deltas in radians.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_orbit(
    renderer: *mut GsplatSurfaceRenderer,
    delta_yaw_radians: f32,
    delta_pitch_radians: f32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_orbit", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_orbit: renderer is null",
                );
            }
        };

        if !delta_yaw_radians.is_finite() || !delta_pitch_radians.is_finite() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_orbit: deltas must be finite",
            );
        }

        renderer.camera_control.yaw += delta_yaw_radians;
        renderer.camera_control.pitch = (renderer.camera_control.pitch + delta_pitch_radians)
            .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
        apply_surface_camera_control(renderer)
    })
}

/// Zoom the Surface camera by a positive distance scale.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_zoom(
    renderer: *mut GsplatSurfaceRenderer,
    distance_scale: f32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_zoom", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_zoom: renderer is null",
                );
            }
        };

        if !distance_scale.is_finite() || distance_scale <= 0.0 {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_zoom: distance_scale must be finite and positive",
            );
        }

        let min_distance =
            (renderer.camera_control.radius * SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER).max(0.01);
        let max_distance = (renderer.camera_control.radius
            * SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER)
            .max(min_distance);
        renderer.camera_control.distance =
            (renderer.camera_control.distance * distance_scale).clamp(min_distance, max_distance);
        apply_surface_camera_control(renderer)
    })
}

/// Pan the Surface camera in normalized viewport units.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_pan(
    renderer: *mut GsplatSurfaceRenderer,
    normalized_delta_x: f32,
    normalized_delta_y: f32,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_pan", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_pan: renderer is null",
                );
            }
        };

        if !normalized_delta_x.is_finite() || !normalized_delta_y.is_finite() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_pan: deltas must be finite",
            );
        }

        let config = renderer.session.renderer().config();
        let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
        let view_height = 2.0
            * renderer.camera_control.distance
            * (renderer.session.camera().intrinsics.vertical_fov_radians * 0.5).tan();
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
    })
}

/// Render one frame to the Surface renderer.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_render_frame(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_render_frame", || {
        let renderer = match unsafe { renderer.as_mut() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_render_frame: renderer is null",
                );
            }
        };

        match renderer.session.render_frame() {
            Ok(output) => {
                renderer.last_sort_stats = output.into();
                renderer.render_error_logged = false;
                ffi_ok()
            }
            Err(err) => {
                if !renderer.render_error_logged {
                    log_surface_error(&format!(
                        "gsplat_surface_renderer_render_frame failed: {err:?}"
                    ));
                    renderer.render_error_logged = true;
                }
                ffi_error_display(err.code(), "gsplat_surface_renderer_render_frame", err)
            }
        }
    })
}

/// Copy the last Surface renderer stats.
///
/// # Safety
///
/// `renderer` must be null or a live handle returned by a Surface renderer
/// create function. `out_stats` must be valid for one `GsplatStats` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_stats(
    renderer: *const GsplatSurfaceRenderer,
    out_stats: *mut GsplatStats,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_stats", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_stats: renderer is null",
                );
            }
        };

        if out_stats.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_stats: out_stats is null",
            );
        }

        unsafe {
            *out_stats = renderer.session.last_stats().into();
        }

        ffi_ok()
    })
}

/// Copy bounded async-sort telemetry for the last Surface frame.
///
/// # Safety
///
/// `renderer` must be null or a live Surface renderer. `out_stats` must be
/// valid for one `GsplatSurfaceSortStats` write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_get_sort_stats(
    renderer: *const GsplatSurfaceRenderer,
    out_stats: *mut GsplatSurfaceSortStats,
) -> i32 {
    ffi_catch_i32("gsplat_surface_renderer_get_sort_stats", || {
        let renderer = match unsafe { renderer.as_ref() } {
            Some(renderer) => renderer,
            None => {
                return ffi_error(
                    ErrorCode::InvalidArgument,
                    "gsplat_surface_renderer_get_sort_stats: renderer is null",
                );
            }
        };
        if out_stats.is_null() {
            return ffi_error(
                ErrorCode::InvalidArgument,
                "gsplat_surface_renderer_get_sort_stats: out_stats is null",
            );
        }
        unsafe {
            *out_stats = renderer.last_sort_stats;
        }
        ffi_ok()
    })
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
    let camera = surface_camera_from_control(
        renderer.camera_control,
        renderer.session.renderer().config(),
    );
    if let Err(err) = renderer.session.set_camera(camera) {
        return ffi_error_display(err.code(), "apply_surface_camera_control", err);
    }

    renderer.render_error_logged = false;
    ffi_ok()
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
    use gsplat_render_wgpu::GeometryPath;

    use super::{
        GsplatCamera, GsplatConfig, GsplatContext, SurfaceCameraControl,
        camera_rotation_looking_at, ffi_catch_i32, geometry_path_from_ffi, gsplat_camera_default,
        gsplat_config_default, gsplat_context_create, gsplat_context_destroy,
        gsplat_context_get_stats, gsplat_context_load_scene_path, gsplat_context_render_frame,
        gsplat_context_set_auto_camera, gsplat_context_set_camera, gsplat_error_message,
        gsplat_last_error_message, gsplat_surface_renderer_get_stats,
        gsplat_surface_renderer_orbit, gsplat_surface_renderer_pan,
        gsplat_surface_renderer_render_frame, gsplat_surface_renderer_reset_camera,
        gsplat_surface_renderer_resize, gsplat_surface_renderer_set_async_geometry,
        gsplat_surface_renderer_set_async_sort, gsplat_surface_renderer_set_frame_latency,
        gsplat_surface_renderer_set_geometry_path, gsplat_surface_renderer_set_gpu_preproject,
        gsplat_surface_renderer_set_gpu_preproject_double_buffer,
        gsplat_surface_renderer_set_instance_buffer_count,
        gsplat_surface_renderer_set_sort_interval, gsplat_surface_renderer_zoom,
        surface_camera_from_control,
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
    fn geometry_path_ids_cover_the_experimental_constructor_contract() {
        assert_eq!(
            geometry_path_from_ffi(0),
            Some(GeometryPath::SortedIndexDirect)
        );
        assert_eq!(geometry_path_from_ffi(1), Some(GeometryPath::PackedAtlas));
        assert_eq!(
            geometry_path_from_ffi(2),
            Some(GeometryPath::PagedActiveAtlas)
        );
        assert_eq!(geometry_path_from_ffi(3), None);
    }

    #[test]
    fn error_message_describes_known_and_unknown_codes() {
        let invalid = unsafe { std::ffi::CStr::from_ptr(gsplat_error_message(1)) };
        assert_eq!(invalid.to_str().unwrap(), "invalid argument");

        let unknown = unsafe { std::ffi::CStr::from_ptr(gsplat_error_message(-1)) };
        assert_eq!(unknown.to_str().unwrap(), "unknown error");
    }

    #[test]
    fn last_error_message_tracks_recent_failure() {
        let rc = unsafe { gsplat_context_load_scene_path(ptr::null_mut(), ptr::null()) };
        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());

        let detail = unsafe { std::ffi::CStr::from_ptr(gsplat_last_error_message()) };
        assert!(
            detail
                .to_str()
                .unwrap()
                .contains("gsplat_context_load_scene_path")
        );
    }

    #[test]
    fn panic_at_ffi_boundary_returns_internal_error_and_detail() {
        let rc = ffi_catch_i32("gsplat_test_panicking_entrypoint", || {
            panic!("simulated internal panic")
        });

        assert_eq!(rc, ErrorCode::Internal.as_i32());
        let detail = unsafe { std::ffi::CStr::from_ptr(gsplat_last_error_message()) };
        assert_eq!(
            detail.to_str().unwrap(),
            "gsplat_test_panicking_entrypoint: panic caught at C ABI boundary"
        );
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
    fn context_create_rejects_null_out_pointer() {
        let rc = unsafe { gsplat_context_create(GsplatConfig::default(), ptr::null_mut()) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
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

    #[test]
    fn context_functions_reject_null_handles_and_outputs() {
        let mut stats = super::GsplatStats::from(gsplat_core::FrameStats::zero());

        assert_eq!(
            unsafe { gsplat_context_set_auto_camera(ptr::null_mut()) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_load_scene_path(ptr::null_mut(), ptr::null()) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_render_frame(ptr::null_mut()) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_get_stats(ptr::null(), &mut stats) },
            ErrorCode::InvalidArgument.as_i32()
        );
        assert_eq!(
            unsafe { gsplat_context_get_stats(ptr::null(), ptr::null_mut()) },
            ErrorCode::InvalidArgument.as_i32()
        );
    }

    #[test]
    fn context_load_scene_path_rejects_null_path() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let create_rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(create_rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        let rc = unsafe { gsplat_context_load_scene_path(ctx, ptr::null()) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn context_get_stats_rejects_null_output() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();
        let create_rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(create_rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        let rc = unsafe { gsplat_context_get_stats(ctx, ptr::null_mut()) };

        assert_eq!(rc, ErrorCode::InvalidArgument.as_i32());
        unsafe { gsplat_context_destroy(ctx) };
    }

    #[test]
    fn surface_functions_reject_null_renderer() {
        let mut stats = super::GsplatStats::from(gsplat_core::FrameStats::zero());
        let expected = ErrorCode::InvalidArgument.as_i32();

        assert_eq!(
            unsafe { gsplat_surface_renderer_resize(ptr::null_mut(), 640, 480) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_sort_interval(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_gpu_preproject(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_gpu_preproject_double_buffer(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_async_sort(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_async_geometry(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_geometry_path(ptr::null_mut(), 1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_instance_buffer_count(ptr::null_mut(), 2) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_set_frame_latency(ptr::null_mut(), 2) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_reset_camera(ptr::null_mut()) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_orbit(ptr::null_mut(), 0.1, 0.1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_zoom(ptr::null_mut(), 1.1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_pan(ptr::null_mut(), 0.1, 0.1) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_render_frame(ptr::null_mut()) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_stats(ptr::null(), &mut stats) },
            expected
        );
        assert_eq!(
            unsafe { gsplat_surface_renderer_get_stats(ptr::null(), ptr::null_mut()) },
            expected
        );
    }
}
