use std::path::Path;

use gsplat_core::{Camera, RendererConfig, Vec3f};
use gsplat_io_ply::{DecodedPlySplat, load_ply, load_ply_summary, visit_ply_splats};
use gsplat_render_wgpu::{GeometryPath, Renderer, ResidentSceneBuilder, ResidentSourceSplat};

use crate::cli::Args;
use crate::trace::CameraTracePlayback;

pub(crate) fn load_ply_path_into_renderer(
    path: &Path,
    renderer: &mut Renderer,
) -> Result<(), String> {
    if renderer.geometry_path() != GeometryPath::PackedAtlas {
        let loaded = load_ply(path).map_err(|error| error.to_string())?;
        return renderer
            .load_scene(loaded.scene)
            .map_err(|error| error.to_string());
    }

    let expected = load_ply_summary(path).map_err(|error| error.to_string())?;
    let mut builder = ResidentSceneBuilder::new(expected.gaussians, expected.sh_degree)
        .map_err(|error| error.to_string())?;
    let mut builder_error = None;
    let decoded = visit_ply_splats(path, |splat| {
        if builder_error.is_none() {
            builder_error = builder.push(resident_source_splat(splat)).err();
        }
    })
    .map_err(|error| error.to_string())?;
    if let Some(error) = builder_error {
        return Err(error.to_string());
    }
    if decoded != expected {
        return Err(format!(
            "PLY changed while loading: expected {expected:?}, decoded {decoded:?}"
        ));
    }
    let resident = builder.finish().map_err(|error| error.to_string())?;
    renderer
        .load_resident_scene(resident)
        .map_err(|error| error.to_string())
}

fn resident_source_splat(splat: &DecodedPlySplat) -> ResidentSourceSplat {
    ResidentSourceSplat {
        position: splat.position_ruf,
        opacity_logit: splat.opacity_logit,
        log_scale: splat.log_scale_xyz,
        rotation_xyzw: splat.rotation_xyzw,
        color_dc: splat.color_dc,
        sh_rest: splat.sh_rest,
        sh_len: splat.sh_rest_len,
        sh_degree: splat.sh_degree,
    }
}

pub(crate) fn initial_camera(
    args: &Args,
    renderer: &Renderer,
    trace_playback: Option<&CameraTracePlayback>,
) -> Result<Camera, String> {
    let mut camera = if let Some(playback) = trace_playback {
        playback.initial_camera()?
    } else if args.auto_camera {
        auto_camera(renderer, args.config)
    } else {
        Camera::default()
    };
    if let Some(yaw_deg) = args.yaw_deg {
        let yaw = yaw_deg.to_radians();
        camera.pose.rotation_xyzw = [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
    }
    Ok(camera)
}

pub(crate) fn auto_camera(renderer: &Renderer, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    camera.intrinsics.vertical_fov_radians = 60.0_f32.to_radians();

    let Some(positions) = renderer.positions() else {
        return camera;
    };
    let Some((min, max)) = positions_bounds(positions) else {
        return camera;
    };

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
    // Keep enough standoff for thick scenes; using only x/y fit can place the camera too close
    // to the frontmost Gaussians and amplify projection anisotropy into visible streaking.
    let base_dist = dist_y.max(dist_x);
    let depth_aware_dist = base_dist + half_z;
    let dist = depth_aware_dist * 1.2;
    camera.pose.position = Vec3f::new(center.x, center.y, center.z - dist);
    camera.pose.rotation_xyzw = [0.0, 0.0, 0.0, 1.0];

    // Keep conservative planes to avoid accidental clipping when orbiting.
    let radius = half_x.max(half_y).max((extent.z * 0.5).max(1e-3));
    camera.intrinsics.near_plane = (dist - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (dist + radius * 8.0).max(100.0);

    camera
}

#[cfg(test)]
pub(crate) fn scene_bounds(scene: &gsplat_core::SceneBuffers) -> Option<(Vec3f, Vec3f)> {
    positions_bounds(&scene.positions)
}

fn positions_bounds(positions: &[Vec3f]) -> Option<(Vec3f, Vec3f)> {
    if positions.is_empty() {
        return None;
    }
    let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }
    Some((min, max))
}

#[cfg(feature = "interactive-viewer")]
pub(crate) fn positions_center(positions: &[Vec3f]) -> Option<Vec3f> {
    let (min, max) = positions_bounds(positions)?;
    Some(Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    ))
}
