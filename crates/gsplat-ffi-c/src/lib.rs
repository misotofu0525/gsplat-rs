//! Stable C ABI surface for mobile wrappers.

use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::ptr::NonNull;

use gsplat_core::{
    Camera, CameraIntrinsics, CameraPose, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR,
    GSPLAT_API_VERSION_MINOR, RenderMode, RendererConfig, Vec3f,
};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::{GpuInstance, Renderer, SURFACE_INSTANCE_LIMIT, SurfacePresenter};

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
    camera: Camera,
    surface_stats: FrameStats,
    uploaded_frame: bool,
    render_error_logged: bool,
}

fn log_surface_error(message: &str) {
    eprintln!("gsplat-ffi-c: {message}");
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
    let camera = match auto_camera(&renderer) {
        Ok(camera) => camera,
        Err(code) => return code.as_i32(),
    };

    let surface_renderer = Box::new(GsplatSurfaceRenderer {
        renderer,
        presenter,
        camera,
        surface_stats: FrameStats::zero(),
        uploaded_frame: false,
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
    renderer.camera = match auto_camera(&renderer.renderer) {
        Ok(camera) => camera,
        Err(code) => return code.as_i32(),
    };
    renderer.surface_stats = FrameStats::zero();
    renderer.uploaded_frame = false;
    renderer.render_error_logged = false;

    ErrorCode::Ok.as_i32()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsplat_surface_renderer_render_frame(
    renderer: *mut GsplatSurfaceRenderer,
) -> i32 {
    let renderer = match unsafe { renderer.as_mut() } {
        Some(renderer) => renderer,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let result = if renderer.uploaded_frame {
        renderer.presenter.render_current().map(|_| None)
    } else {
        let (instances, mut stats) =
            match renderer.renderer.build_sorted_instances(&renderer.camera) {
                Ok(result) => result,
                Err(err) => return err.code().as_i32(),
            };
        let instances = limit_surface_instances(instances);
        stats.drawn_count = instances.len() as u32;
        renderer
            .presenter
            .render_instances(&instances)
            .map(|_| Some(stats))
    };

    match result {
        Ok(stats) => {
            if let Some(stats) = stats {
                renderer.surface_stats = stats;
            }
            renderer.uploaded_frame = true;
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

fn limit_surface_instances(instances: Vec<GpuInstance>) -> Vec<GpuInstance> {
    if instances.len() <= SURFACE_INSTANCE_LIMIT {
        return instances;
    }

    let len = instances.len();
    let step = len as f64 / SURFACE_INSTANCE_LIMIT as f64;
    (0..SURFACE_INSTANCE_LIMIT)
        .map(|i| {
            let index = ((i as f64) * step).floor() as usize;
            instances[index.min(len - 1)]
        })
        .collect()
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

fn auto_camera(renderer: &Renderer) -> Result<Camera, ErrorCode> {
    let Some(scene) = renderer.scene() else {
        return Err(ErrorCode::SceneNotLoaded);
    };
    let Some((min, max)) = scene_bounds(scene) else {
        return Err(ErrorCode::InvalidArgument);
    };

    let config = renderer.config();
    let mut camera = Camera::default();
    camera.intrinsics.vertical_fov_radians = 60.0_f32.to_radians();

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
    let vfov = camera.intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    let dist = (dist_y.max(dist_x) + half_z) * 1.2;
    camera.pose.position = Vec3f::new(center.x, center.y, center.z - dist);
    camera.pose.rotation_xyzw = [0.0, 0.0, 0.0, 1.0];

    let radius = half_x.max(half_y).max(half_z);
    camera.intrinsics.near_plane = (dist - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (dist + radius * 8.0).max(100.0);

    Ok(camera)
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
        GsplatCamera, GsplatConfig, GsplatContext, gsplat_context_create, gsplat_context_destroy,
        gsplat_context_set_camera,
    };

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
