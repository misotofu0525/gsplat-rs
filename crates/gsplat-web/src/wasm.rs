use gsplat_core::{
    Camera, ErrorCode, FrameStats, GSPLAT_API_VERSION_MAJOR, GSPLAT_API_VERSION_MINOR, RenderMode,
    RendererConfig, SceneBuffers, Vec3f,
};
use gsplat_io_ply::{PlySceneSummary, parse_ply_bytes};
use gsplat_render_wgpu::{Renderer, SurfaceFrameTimings, SurfacePresenter, SurfaceRenderSession};
use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

const SURFACE_CAMERA_MAX_PITCH: f32 = 1.45;
const SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER: f32 = 0.2;
const SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER: f32 = 20.0;
#[wasm_bindgen]
pub fn api_version_major() -> u32 {
    GSPLAT_API_VERSION_MAJOR
}

#[wasm_bindgen]
pub fn api_version_minor() -> u32 {
    GSPLAT_API_VERSION_MINOR
}

#[wasm_bindgen(js_name = createRenderer)]
pub async fn create_renderer(
    canvas: HtmlCanvasElement,
    ply_bytes: Uint8Array,
    width: u32,
    height: u32,
) -> Result<GsplatWebRenderer, JsValue> {
    create_renderer_with_options(canvas, ply_bytes, width, height, false).await
}

#[wasm_bindgen(js_name = createRendererWithOptions)]
pub async fn create_renderer_with_options(
    canvas: HtmlCanvasElement,
    ply_bytes: Uint8Array,
    width: u32,
    height: u32,
    _sorted_index_direct: bool,
) -> Result<GsplatWebRenderer, JsValue> {
    let raw = ply_bytes.to_vec();
    let loaded = parse_ply_bytes(&raw).map_err(|err| js_error(err.to_string()))?;
    let summary = loaded.summary;
    let mut renderer = Renderer::with_config_for_surface(RendererConfig {
        width,
        height,
        mode: RenderMode::SortedAlpha,
    })
    .map_err(renderer_error)?;
    renderer.load_scene(loaded.scene).map_err(renderer_error)?;

    let presenter = SurfacePresenter::from_canvas(canvas, width, height, &renderer)
        .await
        .map_err(|err| js_error(err.to_string()))?;
    let (surface_width, surface_height) = presenter.surface_size();
    renderer
        .set_size(surface_width, surface_height)
        .map_err(renderer_error)?;

    let camera_control = auto_surface_camera_control(&renderer).map_err(error_code)?;
    let camera = surface_camera_from_control(camera_control, renderer.config());
    let session = SurfaceRenderSession::new(renderer, presenter, camera).map_err(renderer_error)?;

    Ok(GsplatWebRenderer {
        session,
        camera_control,
        summary,
    })
}

#[wasm_bindgen]
pub struct GsplatWebRenderer {
    session: SurfaceRenderSession,
    camera_control: SurfaceCameraControl,
    summary: PlySceneSummary,
}

#[wasm_bindgen]
impl GsplatWebRenderer {
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        self.session.resize(width, height).map_err(renderer_error)?;
        let camera =
            surface_camera_from_control(self.camera_control, self.session.renderer().config());
        self.session.set_camera(camera).map_err(renderer_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = resetCamera)]
    pub fn reset_camera(&mut self) -> Result<(), JsValue> {
        self.camera_control =
            auto_surface_camera_control(self.session.renderer()).map_err(error_code)?;
        self.apply_camera_control()?;
        self.session.force_sort_refresh();
        Ok(())
    }

    pub fn orbit(
        &mut self,
        delta_yaw_radians: f32,
        delta_pitch_radians: f32,
    ) -> Result<(), JsValue> {
        if !delta_yaw_radians.is_finite() || !delta_pitch_radians.is_finite() {
            return Err(error_code(ErrorCode::InvalidArgument));
        }

        self.camera_control.yaw += delta_yaw_radians;
        self.camera_control.pitch = (self.camera_control.pitch + delta_pitch_radians)
            .clamp(-SURFACE_CAMERA_MAX_PITCH, SURFACE_CAMERA_MAX_PITCH);
        self.apply_camera_control()
    }

    pub fn zoom(&mut self, distance_scale: f32) -> Result<(), JsValue> {
        if !distance_scale.is_finite() || distance_scale <= 0.0 {
            return Err(error_code(ErrorCode::InvalidArgument));
        }

        let min_distance =
            (self.camera_control.radius * SURFACE_CAMERA_MIN_DISTANCE_MULTIPLIER).max(0.01);
        let max_distance =
            (self.camera_control.radius * SURFACE_CAMERA_MAX_DISTANCE_MULTIPLIER).max(min_distance);
        self.camera_control.distance =
            (self.camera_control.distance * distance_scale).clamp(min_distance, max_distance);
        self.apply_camera_control()
    }

    pub fn pan(&mut self, normalized_delta_x: f32, normalized_delta_y: f32) -> Result<(), JsValue> {
        if !normalized_delta_x.is_finite() || !normalized_delta_y.is_finite() {
            return Err(error_code(ErrorCode::InvalidArgument));
        }

        let config = self.session.renderer().config();
        let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
        let view_height = 2.0
            * self.camera_control.distance
            * (self.session.camera().intrinsics.vertical_fov_radians * 0.5).tan();
        let view_width = view_height * aspect;
        let camera = surface_camera_from_control(self.camera_control, config);
        let (right, up, _) = camera_basis(&camera, self.camera_control.target);

        self.camera_control.target = vec3_add(
            self.camera_control.target,
            vec3_add(
                vec3_scale(right, -normalized_delta_x * view_width),
                vec3_scale(up, normalized_delta_y * view_height),
            ),
        );
        self.apply_camera_control()
    }

    #[wasm_bindgen(js_name = setSortInterval)]
    pub fn set_sort_interval(&mut self, interval: u32) {
        // Preserve the existing Web API behavior by clamping zero to one.
        let _ = self.session.set_sort_interval(interval.max(1));
    }

    #[wasm_bindgen(js_name = setSortedIndexDirect)]
    pub fn set_sorted_index_direct(&mut self, _enabled: bool) {}

    #[wasm_bindgen(js_name = sortedIndexDirect)]
    pub fn sorted_index_direct(&self) -> bool {
        true
    }

    #[wasm_bindgen(js_name = rasterPath)]
    pub fn raster_path(&self) -> String {
        "sorted_index_direct".to_owned()
    }

    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&mut self) -> Result<JsValue, JsValue> {
        let output = self.session.render_frame().map_err(renderer_error)?;
        frame_stats_object(
            output.stats,
            output.timings,
            output.sort_refreshed,
            self.session.surface_size(),
        )
    }

    #[wasm_bindgen(js_name = sceneSummary)]
    pub fn scene_summary(&self) -> Result<JsValue, JsValue> {
        let object = Object::new();
        set_u32(&object, "gaussians", self.summary.gaussians as u32)?;
        set_u32(&object, "shDegree", self.summary.sh_degree as u32)?;
        set_bool(&object, "hasShRest", self.summary.has_sh_rest)?;
        Ok(object.into())
    }

    #[wasm_bindgen(js_name = surfaceSize)]
    pub fn surface_size(&self) -> Result<JsValue, JsValue> {
        let (width, height) = self.session.surface_size();
        let object = Object::new();
        set_u32(&object, "width", width)?;
        set_u32(&object, "height", height)?;
        Ok(object.into())
    }
}

impl GsplatWebRenderer {
    fn apply_camera_control(&mut self) -> Result<(), JsValue> {
        let camera =
            surface_camera_from_control(self.camera_control, self.session.renderer().config());
        self.session.set_camera(camera).map_err(renderer_error)
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

    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
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

fn scene_bounds(scene: &SceneBuffers) -> Option<(Vec3f, Vec3f)> {
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

fn frame_stats_object(
    stats: FrameStats,
    timings: SurfaceFrameTimings,
    refresh_sort: bool,
    surface_size: (u32, u32),
) -> Result<JsValue, JsValue> {
    let object = Object::new();
    set_f32(&object, "frameMs", stats.frame_ms)?;
    set_f32(&object, "preprocessMs", stats.preprocess_ms)?;
    set_f32(&object, "sortMs", stats.sort_ms)?;
    set_f32(&object, "rasterMs", stats.raster_ms)?;
    set_f32(&object, "cpuGeometryMs", timings.cpu_geometry_ms)?;
    set_f32(&object, "renderSubmitMs", timings.render_submit_ms)?;
    set_f32(&object, "frameWallMs", timings.frame_wall_ms)?;
    set_u32(&object, "visibleCount", stats.visible_count)?;
    set_u32(&object, "drawnCount", stats.drawn_count)?;
    set_bool(&object, "refreshSort", refresh_sort)?;
    set_u32(&object, "surfaceWidth", surface_size.0)?;
    set_u32(&object, "surfaceHeight", surface_size.1)?;
    Ok(object.into())
}

fn set_f32(object: &Object, key: &str, value: f32) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &JsValue::from_f64(value as f64),
    )
    .map(|_| ())
}

fn set_u32(object: &Object, key: &str, value: u32) -> Result<(), JsValue> {
    Reflect::set(
        object,
        &JsValue::from_str(key),
        &JsValue::from_f64(value as f64),
    )
    .map(|_| ())
}

fn set_bool(object: &Object, key: &str, value: bool) -> Result<(), JsValue> {
    Reflect::set(object, &JsValue::from_str(key), &JsValue::from_bool(value)).map(|_| ())
}

fn renderer_error(err: gsplat_render_wgpu::RendererError) -> JsValue {
    js_error(err.to_string())
}

fn error_code(code: ErrorCode) -> JsValue {
    js_error(format!("{code:?}"))
}

fn js_error(message: impl Into<String>) -> JsValue {
    js_sys::Error::new(&message.into()).into()
}
