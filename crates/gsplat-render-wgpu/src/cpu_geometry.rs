//! CPU-projected GpuInstance oracle used by conformance tests.

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, RendererConfig, SceneBuffers, Vec3f};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::math::{CameraCovarianceTerms, quat_inverse, quat_to_mat3};
use crate::preprocess::{is_visible, world_to_camera_with_view_rot};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuInstance {
    // xy = center in NDC, zw = major axis in NDC
    pub center_and_axis_u: [f32; 4],
    // xy = minor axis in NDC, zw = reserved
    pub axis_v_and_pad: [f32; 4],
    // Premultiplied RGB + alpha
    pub color_rgba: [f32; 4],
}

pub(crate) fn build_instances(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
) -> Vec<GpuInstance> {
    let mut out = Vec::new();
    build_instances_into(
        scene,
        world_covariances,
        alpha_values,
        indices,
        camera,
        config,
        &mut out,
    );
    out
}

pub(crate) fn build_instances_into(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
    out: &mut Vec<GpuInstance>,
) {
    if world_covariances.len() != scene.len() || alpha_values.len() != scene.len() {
        out.clear();
        return;
    }

    let Some(params) = InstanceBuildParams::new(camera, config) else {
        out.clear();
        return;
    };

    if out.len() < indices.len() {
        out.resize(indices.len(), GpuInstance::zeroed());
    } else {
        out.truncate(indices.len());
    }

    let sh_layout = ShColorLayout::new(scene);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let had_invalid = AtomicBool::new(false);
        out.par_iter_mut()
            .zip(indices.par_iter())
            .for_each(|(slot, &idx)| {
                let i = idx as usize;
                let instance = if i < scene.len() {
                    // SAFETY: the explicit bounds check above covers all scene-parallel arrays
                    // because the caller validated equal lengths before entering this loop.
                    unsafe {
                        build_instance_unchecked(
                            scene,
                            world_covariances,
                            alpha_values,
                            i,
                            camera,
                            &params,
                            sh_layout,
                        )
                    }
                } else {
                    None
                };
                if let Some(instance) = instance {
                    *slot = instance;
                } else {
                    *slot = invalid_gpu_instance();
                    had_invalid.store(true, Ordering::Relaxed);
                }
            });

        if had_invalid.load(Ordering::Relaxed) {
            out.retain(|instance| instance.color_rgba[3] >= 0.0);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut write_index = 0_usize;
        for &idx in indices {
            let i = idx as usize;
            let instance = if i < scene.len() {
                // SAFETY: the explicit bounds check above covers all scene-parallel arrays
                // because the caller validated equal lengths before entering this loop.
                unsafe {
                    build_instance_unchecked(
                        scene,
                        world_covariances,
                        alpha_values,
                        i,
                        camera,
                        &params,
                        sh_layout,
                    )
                }
            } else {
                None
            };
            if let Some(instance) = instance {
                out[write_index] = instance;
                write_index += 1;
            }
        }
        out.truncate(write_index);
    }
}

unsafe fn build_instance_unchecked(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    i: usize,
    camera: &Camera,
    params: &InstanceBuildParams,
    sh_layout: ShColorLayout<'_>,
) -> Option<GpuInstance> {
    let pos_world = unsafe { *scene.positions.get_unchecked(i) };
    let p_cam = world_to_camera_with_view_rot(pos_world, camera.pose.position, params.view_rot);
    // Preprocess already culled by z range, but keep this safe for runtime camera changes.
    if !is_visible(p_cam.z, camera) {
        return None;
    }

    // Project center to NDC for instance placement.
    let inv_z = 1.0 / p_cam.z.max(1e-6);
    let x_ndc = (p_cam.x * params.f) * inv_z / params.aspect;
    let y_ndc = (p_cam.y * params.f) * inv_z;

    let cov2_ndc = project_world_covariance_to_ndc(
        p_cam,
        unsafe { *world_covariances.get_unchecked(i) },
        params,
    )?;
    let (axis_u, axis_v) = ellipse_axes_from_covariance(cov2_ndc)?;
    let extent_x = axis_u[0].abs() + axis_v[0].abs();
    let extent_y = axis_u[1].abs() + axis_v[1].abs();
    if x_ndc + extent_x < -1.0
        || x_ndc - extent_x > 1.0
        || y_ndc + extent_y < -1.0
        || y_ndc - extent_y > 1.0
    {
        return None;
    }

    let alpha = unsafe { *alpha_values.get_unchecked(i) };
    let dir_world = normalize3(Vec3f::new(
        pos_world.x - camera.pose.position.x,
        pos_world.y - camera.pose.position.y,
        pos_world.z - camera.pose.position.z,
    ));
    let rgb = unsafe { sh_color_unchecked(scene, i, dir_world, sh_layout) };
    let rgb = [
        rgb[0].clamp(0.0, 1.0),
        rgb[1].clamp(0.0, 1.0),
        rgb[2].clamp(0.0, 1.0),
    ];

    Some(GpuInstance {
        center_and_axis_u: [x_ndc, y_ndc, axis_u[0], axis_u[1]],
        axis_v_and_pad: [axis_v[0], axis_v[1], 0.0, 0.0],
        color_rgba: [
            (rgb[0] * alpha).clamp(0.0, 1.0),
            (rgb[1] * alpha).clamp(0.0, 1.0),
            (rgb[2] * alpha).clamp(0.0, 1.0),
            alpha,
        ],
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn invalid_gpu_instance() -> GpuInstance {
    GpuInstance {
        color_rgba: [0.0, 0.0, 0.0, -1.0],
        ..GpuInstance::zeroed()
    }
}

#[derive(Clone, Copy)]
struct InstanceBuildParams {
    aspect: f32,
    f: f32,
    fx: f32,
    fy: f32,
    lim_x: f32,
    lim_y: f32,
    blur_cov_x: f32,
    blur_cov_y: f32,
    view_rot: [[f32; 3]; 3],
}

impl InstanceBuildParams {
    fn new(camera: &Camera, config: RendererConfig) -> Option<Self> {
        if config.width == 0 || config.height == 0 {
            return None;
        }
        let tan_half_fovy = (camera.intrinsics.vertical_fov_radians * 0.5).tan();
        if tan_half_fovy <= 0.0 || !tan_half_fovy.is_finite() {
            return None;
        }

        let aspect = config.width as f32 / config.height as f32;
        let f = 1.0 / tan_half_fovy;
        let fx = f / aspect;
        let fy = f;
        let tan_half_fovx = tan_half_fovy * aspect;
        let blur_pixels = 0.3_f32;
        let px_ndc_x = 2.0 / config.width as f32;
        let px_ndc_y = 2.0 / config.height as f32;
        let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);

        Some(Self {
            aspect,
            f,
            fx,
            fy,
            lim_x: 1.3 * tan_half_fovx,
            lim_y: 1.3 * tan_half_fovy,
            blur_cov_x: (blur_pixels * px_ndc_x).powi(2),
            blur_cov_y: (blur_pixels * px_ndc_y).powi(2),
            view_rot: quat_to_mat3(camera_inv_q),
        })
    }
}

pub(crate) fn project_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: [[f32; 3]; 3],
    camera: &Camera,
    config: RendererConfig,
) -> Option<[[f32; 2]; 2]> {
    let params = InstanceBuildParams::new(camera, config)?;
    project_camera_covariance_to_ndc(p_cam, CameraCovarianceTerms::from_matrix(cov_cam), &params)
}

fn project_world_covariance_to_ndc(
    p_cam: Vec3f,
    world_cov: [[f32; 3]; 3],
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let world_cov = CameraCovarianceTerms::from_matrix(world_cov);
    project_world_covariance_terms_to_ndc(p_cam, world_cov, params)
}

fn project_world_covariance_terms_to_ndc(
    p_cam: Vec3f,
    world_cov: CameraCovarianceTerms,
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let cov_cam = transform_covariance_terms_to_camera(world_cov, params.view_rot);
    project_camera_covariance_to_ndc(p_cam, cov_cam, params)
}

fn project_camera_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: CameraCovarianceTerms,
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let z = p_cam.z;
    if z <= 1e-6 || !z.is_finite() {
        return None;
    }

    // Match common 3DGS covariance projection behavior: clamp view-space x/z and y/z
    // before Jacobian evaluation to avoid extreme derivatives at frustum edges.
    let x_clamped = (p_cam.x / z).clamp(-params.lim_x, params.lim_x) * z;
    let y_clamped = (p_cam.y / z).clamp(-params.lim_y, params.lim_y) * z;

    let inv_z = 1.0 / z;
    let inv_z2 = inv_z * inv_z;
    let j00 = params.fx * inv_z;
    let j02 = -params.fx * x_clamped * inv_z2;
    let j11 = params.fy * inv_z;
    let j12 = -params.fy * y_clamped * inv_z2;

    let cov01 = j00 * j11 * cov_cam.xy
        + j00 * j12 * cov_cam.xz
        + j02 * j11 * cov_cam.yz
        + j02 * j12 * cov_cam.zz;
    let mut cov2 = [
        [
            j00 * j00 * cov_cam.xx + 2.0 * j00 * j02 * cov_cam.xz + j02 * j02 * cov_cam.zz,
            cov01,
        ],
        [
            cov01,
            j11 * j11 * cov_cam.yy + 2.0 * j11 * j12 * cov_cam.yz + j12 * j12 * cov_cam.zz,
        ],
    ];

    // Low-pass filter in NDC space to keep splats from collapsing to subpixel noise.
    cov2[0][0] += params.blur_cov_x;
    cov2[1][1] += params.blur_cov_y;

    if !cov2[0][0].is_finite()
        || !cov2[0][1].is_finite()
        || !cov2[1][0].is_finite()
        || !cov2[1][1].is_finite()
    {
        return None;
    }

    Some(cov2)
}

fn transform_covariance_terms_to_camera(
    cov: CameraCovarianceTerms,
    view_rot: [[f32; 3]; 3],
) -> CameraCovarianceTerms {
    let c00 = cov.xx;
    let c01 = cov.xy;
    let c02 = cov.xz;
    let c11 = cov.yy;
    let c12 = cov.yz;
    let c22 = cov.zz;
    let r0 = view_rot[0];
    let r1 = view_rot[1];
    let r2 = view_rot[2];

    CameraCovarianceTerms {
        xx: covariance_quadratic(c00, c01, c02, c11, c12, c22, r0),
        xy: covariance_bilinear(c00, c01, c02, c11, c12, c22, r0, r1),
        xz: covariance_bilinear(c00, c01, c02, c11, c12, c22, r0, r2),
        yy: covariance_quadratic(c00, c01, c02, c11, c12, c22, r1),
        yz: covariance_bilinear(c00, c01, c02, c11, c12, c22, r1, r2),
        zz: covariance_quadratic(c00, c01, c02, c11, c12, c22, r2),
    }
}

fn covariance_quadratic(
    c00: f32,
    c01: f32,
    c02: f32,
    c11: f32,
    c12: f32,
    c22: f32,
    r: [f32; 3],
) -> f32 {
    r[0] * r[0] * c00
        + 2.0 * r[0] * r[1] * c01
        + 2.0 * r[0] * r[2] * c02
        + r[1] * r[1] * c11
        + 2.0 * r[1] * r[2] * c12
        + r[2] * r[2] * c22
}

#[allow(clippy::too_many_arguments)]
fn covariance_bilinear(
    c00: f32,
    c01: f32,
    c02: f32,
    c11: f32,
    c12: f32,
    c22: f32,
    a: [f32; 3],
    b: [f32; 3],
) -> f32 {
    let bx = c00 * b[0] + c01 * b[1] + c02 * b[2];
    let by = c01 * b[0] + c11 * b[1] + c12 * b[2];
    let bz = c02 * b[0] + c12 * b[1] + c22 * b[2];
    a[0] * bx + a[1] * by + a[2] * bz
}

pub(crate) fn ellipse_axes_from_covariance(cov2: [[f32; 2]; 2]) -> Option<([f32; 2], [f32; 2])> {
    let a = cov2[0][0];
    let b = cov2[0][1];
    let c = cov2[1][1];
    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return None;
    }

    let apco2 = (a + c) * 0.5;
    let amco2 = (a - c) * 0.5;
    let term = (amco2 * amco2 + b * b).sqrt();
    let major = (apco2 + term).max(1e-10);
    let minor = (apco2 - term).max(1e-10);

    let axis_u_dir = if b.abs() > 1e-8 {
        normalize2([b, major - a])
    } else if a >= c {
        [1.0, 0.0]
    } else {
        [0.0, 1.0]
    };
    let axis_v_dir = [-axis_u_dir[1], axis_u_dir[0]];

    // 3-sigma support region for rasterization.
    let radius_k = 3.0_f32;
    let mut major_radius = (major.sqrt() * radius_k).clamp(1e-4, 2.0);
    let minor_radius = (minor.sqrt() * radius_k).clamp(1e-4, 2.0);

    // Guard against extreme anisotropy from unstable covariance outliers. Those splats tend to
    // show up as needle-like streaks while contributing little stable structure.
    const MAX_ANISOTROPY: f32 = 64.0;
    if major_radius > minor_radius * MAX_ANISOTROPY {
        major_radius = minor_radius * MAX_ANISOTROPY;
    }
    let axis_u = [axis_u_dir[0] * major_radius, axis_u_dir[1] * major_radius];
    let axis_v = [axis_v_dir[0] * minor_radius, axis_v_dir[1] * minor_radius];
    if !axis_u[0].is_finite()
        || !axis_u[1].is_finite()
        || !axis_v[0].is_finite()
        || !axis_v[1].is_finite()
    {
        return None;
    }

    Some((axis_u, axis_v))
}

fn normalize3(v: Vec3f) -> [f32; 3] {
    let len2 = v.x * v.x + v.y * v.y + v.z * v.z;
    if len2 <= 0.0 {
        return [0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v.x * inv, v.y * inv, v.z * inv]
}

fn normalize2(v: [f32; 2]) -> [f32; 2] {
    let len2 = v[0] * v[0] + v[1] * v[1];
    if len2 <= 0.0 {
        return [1.0, 0.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v[0] * inv, v[1] * inv]
}

#[derive(Clone, Copy)]
struct ShColorLayout<'a> {
    rest: Option<&'a [f32]>,
    degree: u8,
    per_channel: usize,
    stride: usize,
}

impl<'a> ShColorLayout<'a> {
    fn new(scene: &'a SceneBuffers) -> Self {
        let rest = scene.sh_rest.as_deref();
        let coeff_total = if rest.is_some() {
            (scene.sh_degree as usize + 1).pow(2)
        } else {
            1
        };
        let per_channel = coeff_total.saturating_sub(1);
        Self {
            rest,
            degree: if rest.is_some() { scene.sh_degree } else { 0 },
            per_channel,
            stride: per_channel * 3,
        }
    }
}

unsafe fn sh_color_unchecked(
    scene: &SceneBuffers,
    index: usize,
    dir: [f32; 3],
    layout: ShColorLayout<'_>,
) -> [f32; 3] {
    // The PLY stores SH coefficients as `f_dc_*` + `f_rest_*`. Evaluate as in 3DGS:
    // `rgb = clamp_min(eval_sh(deg, sh, dir) + 0.5, 0.0)`.
    // Reference: graphdeco-inria/gaussian-splatting `utils/sh_utils.py`.
    const C0: f32 = 0.282_094_8_f32;
    let dc = unsafe { *scene.color_dc.get_unchecked(index) };
    let mut rgb = [C0 * dc[0], C0 * dc[1], C0 * dc[2]];

    if let Some(rest) = layout.rest {
        let base = index * layout.stride;
        if layout.degree == 3
            && layout.per_channel == 15
            && let Some(end) = base.checked_add(45)
            && end <= rest.len()
        {
            let sh_rgb = sh_color_rest_deg3(dir, &rest[base..end]);
            return [
                (rgb[0] + sh_rgb[0] + 0.5).max(0.0),
                (rgb[1] + sh_rgb[1] + 0.5).max(0.0),
                (rgb[2] + sh_rgb[2] + 0.5).max(0.0),
            ];
        }

        let (basis, basis_len) = sh_basis(layout.degree, dir);
        for (channel, value) in rgb.iter_mut().enumerate() {
            let channel_base = base + channel * layout.per_channel;
            if channel_base >= rest.len() {
                continue;
            }
            let available = rest.len() - channel_base;
            let term_count = basis_len.min(layout.per_channel).min(available);
            *value += dot_sh_terms(&basis, &rest[channel_base..], term_count);
        }
    }

    [
        (rgb[0] + 0.5).max(0.0),
        (rgb[1] + 0.5).max(0.0),
        (rgb[2] + 0.5).max(0.0),
    ]
}

fn sh_color_rest_deg3(dir: [f32; 3], rest: &[f32]) -> [f32; 3] {
    debug_assert!(rest.len() >= 45);

    const C1: f32 = 0.488_602_52_f32;
    const C2: [f32; 5] = [
        1.092_548_5_f32,
        -1.092_548_5_f32,
        0.315_391_57_f32,
        -1.092_548_5_f32,
        0.546_274_24_f32,
    ];
    const C3: [f32; 7] = [
        -0.590_043_6_f32,
        2.890_611_4_f32,
        -0.457_045_8_f32,
        0.373_176_34_f32,
        -0.457_045_8_f32,
        1.445_305_7_f32,
        -0.590_043_6_f32,
    ];

    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    let b0 = -C1 * y;
    let b1 = C1 * z;
    let b2 = -C1 * x;
    let b3 = C2[0] * xy;
    let b4 = C2[1] * yz;
    let b5 = C2[2] * (2.0 * zz - xx - yy);
    let b6 = C2[3] * xz;
    let b7 = C2[4] * (xx - yy);
    let b8 = C3[0] * y * (3.0 * xx - yy);
    let b9 = C3[1] * xy * z;
    let b10 = C3[2] * y * (4.0 * zz - xx - yy);
    let b11 = C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
    let b12 = C3[4] * x * (4.0 * zz - xx - yy);
    let b13 = C3[5] * z * (xx - yy);
    let b14 = C3[6] * x * (xx - 3.0 * yy);

    [
        sh_dot15(
            rest, b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14,
        ),
        sh_dot15(
            &rest[15..],
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
        ),
        sh_dot15(
            &rest[30..],
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn sh_dot15(
    rest: &[f32],
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
    b7: f32,
    b8: f32,
    b9: f32,
    b10: f32,
    b11: f32,
    b12: f32,
    b13: f32,
    b14: f32,
) -> f32 {
    debug_assert!(rest.len() >= 15);
    b0 * rest[0]
        + b1 * rest[1]
        + b2 * rest[2]
        + b3 * rest[3]
        + b4 * rest[4]
        + b5 * rest[5]
        + b6 * rest[6]
        + b7 * rest[7]
        + b8 * rest[8]
        + b9 * rest[9]
        + b10 * rest[10]
        + b11 * rest[11]
        + b12 * rest[12]
        + b13 * rest[13]
        + b14 * rest[14]
}

fn sh_basis(deg: u8, dir: [f32; 3]) -> ([f32; 24], usize) {
    const C1: f32 = 0.488_602_52_f32;
    const C2: [f32; 5] = [
        1.092_548_5_f32,
        -1.092_548_5_f32,
        0.315_391_57_f32,
        -1.092_548_5_f32,
        0.546_274_24_f32,
    ];
    const C3: [f32; 7] = [
        -0.590_043_6_f32,
        2.890_611_4_f32,
        -0.457_045_8_f32,
        0.373_176_34_f32,
        -0.457_045_8_f32,
        1.445_305_7_f32,
        -0.590_043_6_f32,
    ];
    const C4: [f32; 9] = [
        2.503_342_9_f32,
        -1.770_130_8_f32,
        0.946_174_7_f32,
        -0.669_046_5_f32,
        0.105_785_55_f32,
        -0.669_046_5_f32,
        0.473_087_34_f32,
        -1.770_130_8_f32,
        0.625_835_7_f32,
    ];

    let mut basis = [0.0_f32; 24];
    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    if deg == 0 {
        return (basis, 0);
    }

    basis[0] = -C1 * y;
    basis[1] = C1 * z;
    basis[2] = -C1 * x;
    if deg == 1 {
        return (basis, 3);
    }

    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    basis[3] = C2[0] * xy;
    basis[4] = C2[1] * yz;
    basis[5] = C2[2] * (2.0 * zz - xx - yy);
    basis[6] = C2[3] * xz;
    basis[7] = C2[4] * (xx - yy);
    if deg == 2 {
        return (basis, 8);
    }

    basis[8] = C3[0] * y * (3.0 * xx - yy);
    basis[9] = C3[1] * xy * z;
    basis[10] = C3[2] * y * (4.0 * zz - xx - yy);
    basis[11] = C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
    basis[12] = C3[4] * x * (4.0 * zz - xx - yy);
    basis[13] = C3[5] * z * (xx - yy);
    basis[14] = C3[6] * x * (xx - 3.0 * yy);
    if deg == 3 {
        return (basis, 15);
    }

    // deg 4 support (rare for 3DGS, but safe to handle).
    basis[15] = C4[0] * xy * (xx - yy);
    basis[16] = C4[1] * yz * (3.0 * xx - yy);
    basis[17] = C4[2] * xy * (7.0 * zz - 1.0);
    basis[18] = C4[3] * yz * (7.0 * zz - 3.0);
    basis[19] = C4[4] * (zz * (35.0 * zz - 30.0) + 3.0);
    basis[20] = C4[5] * xz * (7.0 * zz - 3.0);
    basis[21] = C4[6] * (xx - yy) * (7.0 * zz - 1.0);
    basis[22] = C4[7] * xz * (xx - 3.0 * yy);
    basis[23] = C4[8] * (xx * (xx - 3.0 * yy) - yy * (3.0 * xx - yy));
    (basis, 24)
}

fn dot_sh_terms(basis: &[f32; 24], rest: &[f32], count: usize) -> f32 {
    debug_assert!(count <= basis.len());
    debug_assert!(count <= rest.len());

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: AArch64 guarantees Neon availability and the count has been bounds-checked.
        unsafe { dot_sh_terms_neon(basis, rest, count) }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut result = 0.0_f32;
        for i in 0..count {
            result += basis[i] * rest[i];
        }
        result
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn dot_sh_terms_neon(basis: &[f32; 24], rest: &[f32], count: usize) -> f32 {
    use std::arch::aarch64::*;

    let mut sum4 = vdupq_n_f32(0.0);
    let mut i = 0_usize;
    while i + 4 <= count {
        let b = unsafe { vld1q_f32(basis.as_ptr().add(i)) };
        let r = unsafe { vld1q_f32(rest.as_ptr().add(i)) };
        sum4 = vmlaq_f32(sum4, b, r);
        i += 4;
    }

    let mut lanes = [0.0_f32; 4];
    unsafe { vst1q_f32(lanes.as_mut_ptr(), sum4) };
    let mut result = lanes[0] + lanes[1] + lanes[2] + lanes[3];
    while i < count {
        result += unsafe { *basis.as_ptr().add(i) } * unsafe { *rest.as_ptr().add(i) };
        i += 1;
    }
    result
}
