//! CPU visibility filtering and depth-key generation.

use gsplat_core::{Camera, SceneBuffers, Vec3f};

use crate::RendererError;
use crate::math::{quat_inverse, quat_to_mat3};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOutput {
    pub depth_keys: Vec<u32>,
    pub indices: Vec<u32>,
}

pub(crate) fn is_visible(depth_z: f32, camera: &Camera) -> bool {
    depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane
}

fn depth_to_key(depth_z: f32) -> u32 {
    // Positive finite depth values preserve a monotonic relationship when using IEEE-754 bits.
    depth_z.max(0.0).to_bits()
}

pub(crate) fn preprocess_visible_into(
    scene: &SceneBuffers,
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    depth_keys.clear();
    indices.clear();
    if depth_keys.capacity() < scene.len() {
        depth_keys.reserve(scene.len() - depth_keys.capacity());
    }
    if indices.capacity() < scene.len() {
        indices.reserve(scene.len() - indices.capacity());
    }

    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;

    for (idx, position) in scene.positions.iter().enumerate() {
        let depth_z = world_to_camera_depth_with_view_row(*position, camera_position, depth_row);
        if is_visible(depth_z, camera) {
            indices.push(idx as u32);
            depth_keys.push(depth_to_key(depth_z));
        }
    }

    Ok(())
}

pub(crate) fn world_to_camera_depth_with_view_row(
    pos_world: Vec3f,
    camera_position: Vec3f,
    depth_row: [f32; 3],
) -> f32 {
    depth_row[0] * (pos_world.x - camera_position.x)
        + depth_row[1] * (pos_world.y - camera_position.y)
        + depth_row[2] * (pos_world.z - camera_position.z)
}

#[cfg(test)]
pub(crate) fn world_to_camera_with_view_rot(
    pos_world: Vec3f,
    camera_position: Vec3f,
    view_rot: [[f32; 3]; 3],
) -> Vec3f {
    let p = Vec3f::new(
        pos_world.x - camera_position.x,
        pos_world.y - camera_position.y,
        pos_world.z - camera_position.z,
    );

    Vec3f::new(
        view_rot[0][0] * p.x + view_rot[0][1] * p.y + view_rot[0][2] * p.z,
        view_rot[1][0] * p.x + view_rot[1][1] * p.y + view_rot[1][2] * p.z,
        view_rot[2][0] * p.x + view_rot[2][1] * p.y + view_rot[2][2] * p.z,
    )
}
