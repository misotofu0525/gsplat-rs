//! Stable C ABI surface for mobile wrappers.

use std::ffi::{CStr, c_char};
use std::path::Path;

use gsplat_core::{
    Camera, CameraIntrinsics, CameraPose, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR,
    GSPLAT_API_VERSION_MINOR, RenderMode, RendererConfig, Vec3f,
};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::Renderer;

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

    let mode = match RenderMode::from_u32(config.mode) {
        Some(mode) => mode,
        None => return ErrorCode::InvalidArgument.as_i32(),
    };

    let renderer_config = RendererConfig {
        width: config.width,
        height: config.height,
        mode,
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

    ctx.camera = camera.into();
    ErrorCode::Ok.as_i32()
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

#[cfg(test)]
mod tests {
    use std::ptr;

    use gsplat_core::ErrorCode;

    use super::{GsplatConfig, GsplatContext, gsplat_context_create, gsplat_context_destroy};

    #[test]
    fn create_and_destroy_context() {
        let mut ctx: *mut GsplatContext = ptr::null_mut();

        let rc = unsafe { gsplat_context_create(GsplatConfig::default(), &mut ctx) };
        assert_eq!(rc, ErrorCode::Ok.as_i32());
        assert!(!ctx.is_null());

        unsafe { gsplat_context_destroy(ctx) };
    }
}
