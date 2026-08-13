//! CPU-side splat math shared by preprocess, resident upload, and tests.

use gsplat_core::SceneBuffers;

pub(crate) fn precompute_alpha_values(scene: &SceneBuffers) -> Vec<f32> {
    scene
        .opacity
        .iter()
        .map(|&opacity| sigmoid(opacity).clamp(0.0, 1.0))
        .collect()
}

pub(crate) fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

pub(crate) fn quat_inverse(q: [f32; 4]) -> [f32; 4] {
    let q = quat_normalize(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [-x, -y, -z, w]
}

pub(crate) fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let norm2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if norm2 <= 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 / norm2.sqrt();
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

pub(crate) fn quat_to_mat3(q: [f32; 4]) -> [[f32; 3]; 3] {
    let q = quat_normalize(q);
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

pub(crate) fn mat3_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

pub(crate) fn mat3_transpose(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

#[derive(Clone, Copy)]
pub(crate) struct CameraCovarianceTerms {
    pub(crate) xx: f32,
    pub(crate) xy: f32,
    pub(crate) xz: f32,
    pub(crate) yy: f32,
    pub(crate) yz: f32,
    pub(crate) zz: f32,
}

impl CameraCovarianceTerms {
    pub(crate) fn from_matrix(cov: [[f32; 3]; 3]) -> Self {
        Self {
            xx: cov[0][0],
            xy: cov[0][1],
            xz: cov[0][2],
            yy: cov[1][1],
            yz: cov[1][2],
            zz: cov[2][2],
        }
    }
}

pub(crate) fn precompute_world_covariances(scene: &SceneBuffers) -> Vec<[[f32; 3]; 3]> {
    let mut out = Vec::with_capacity(scene.len());
    for i in 0..scene.len() {
        let scale = scene.scale_xyz[i];
        let sx = scale[0].exp().max(1e-6);
        let sy = scale[1].exp().max(1e-6);
        let sz = scale[2].exp().max(1e-6);
        let object_cov = [
            [sx * sx, 0.0, 0.0],
            [0.0, sy * sy, 0.0],
            [0.0, 0.0, sz * sz],
        ];
        let rot_gaussian = quat_to_mat3(quat_normalize(scene.rotation_xyzw[i]));
        let world_cov = mat3_mul(
            mat3_mul(rot_gaussian, object_cov),
            mat3_transpose(rot_gaussian),
        );
        out.push(world_cov);
    }
    out
}
