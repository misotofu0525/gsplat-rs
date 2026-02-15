struct InstanceData {
  center_and_axis_u: vec4<f32>,
  axis_v_and_pad: vec4<f32>,
  color: vec4<f32>,
};

struct SceneElem {
  position: vec4<f32>,
  cov_row0: vec4<f32>,
  cov_row1: vec4<f32>,
  cov_row2: vec4<f32>,
  color_dc: vec4<f32>,
  opacity_and_pad: vec4<f32>,
};

struct Params {
  camera_pos: vec4<f32>,
  camera_inv_q: vec4<f32>,
  view_rot_row0: vec4<f32>,
  view_rot_row1: vec4<f32>,
  view_rot_row2: vec4<f32>,
  vertical_fov_radians: f32,
  near_plane: f32,
  far_plane: f32,
  aspect: f32,
  width: u32,
  height: u32,
  len: u32,
  sh_degree: u32,
};

@group(0) @binding(0)
var<storage, read> sorted_indices: array<u32>;
@group(0) @binding(1)
var<storage, read> scene_elems: array<SceneElem>;
@group(0) @binding(2)
var<storage, read> sh_rest: array<f32>;
@group(0) @binding(3)
var<uniform> params: Params;
@group(0) @binding(4)
var<storage, read_write> instances: array<InstanceData>;

struct Mat3Rows {
  r0: vec3<f32>,
  r1: vec3<f32>,
  r2: vec3<f32>,
};

fn transpose_rows(m: Mat3Rows) -> Mat3Rows {
  return Mat3Rows(
    vec3<f32>(m.r0.x, m.r1.x, m.r2.x),
    vec3<f32>(m.r0.y, m.r1.y, m.r2.y),
    vec3<f32>(m.r0.z, m.r1.z, m.r2.z),
  );
}

fn mat3_mul_rows(a: Mat3Rows, b: Mat3Rows) -> Mat3Rows {
  let b_c0 = vec3<f32>(b.r0.x, b.r1.x, b.r2.x);
  let b_c1 = vec3<f32>(b.r0.y, b.r1.y, b.r2.y);
  let b_c2 = vec3<f32>(b.r0.z, b.r1.z, b.r2.z);
  return Mat3Rows(
    vec3<f32>(dot(a.r0, b_c0), dot(a.r0, b_c1), dot(a.r0, b_c2)),
    vec3<f32>(dot(a.r1, b_c0), dot(a.r1, b_c1), dot(a.r1, b_c2)),
    vec3<f32>(dot(a.r2, b_c0), dot(a.r2, b_c1), dot(a.r2, b_c2)),
  );
}

fn row_mul_mat(v: vec3<f32>, m: Mat3Rows) -> vec3<f32> {
  let c0 = vec3<f32>(m.r0.x, m.r1.x, m.r2.x);
  let c1 = vec3<f32>(m.r0.y, m.r1.y, m.r2.y);
  let c2 = vec3<f32>(m.r0.z, m.r1.z, m.r2.z);
  return vec3<f32>(dot(v, c0), dot(v, c1), dot(v, c2));
}

fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
  let qv = q.xyz;
  let t = cross(qv, v);
  let t2 = t * 2.0;
  let v_wt = v + q.w * t2;
  let c = cross(qv, t2);
  return v_wt + c;
}

fn normalize2_or_default(v: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
  let len2 = dot(v, v);
  if (len2 <= 1e-20) {
    return fallback;
  }
  return v * inverseSqrt(len2);
}

fn normalize3_or_default(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
  let len2 = dot(v, v);
  if (len2 <= 1e-20) {
    return fallback;
  }
  return v * inverseSqrt(len2);
}

fn sh_rest_coeff(idx: u32, channel: u32, coeff: u32, degree: u32) -> f32 {
  if (degree == 0u) {
    return 0.0;
  }
  let coeff_total = (degree + 1u) * (degree + 1u);
  let per_channel = coeff_total - 1u;
  let stride = per_channel * 3u;
  let base = idx * stride + channel * per_channel + coeff;
  return sh_rest[base];
}

fn eval_sh_channel(idx: u32, channel: u32, dc_coeff: f32, dir: vec3<f32>, degree: u32) -> f32 {
  let c0 = 0.28209479177387814;
  let c1 = 0.4886025119029199;
  let c2 = array<f32, 5>(
    1.0925484305920792,
    -1.0925484305920792,
    0.31539156525252005,
    -1.0925484305920792,
    0.5462742152960396,
  );
  let c3 = array<f32, 7>(
    -0.5900435899266435,
    2.890611442640554,
    -0.4570457994644658,
    0.3731763325901154,
    -0.4570457994644658,
    1.445305721320277,
    -0.5900435899266435,
  );
  let c4 = array<f32, 9>(
    2.5033429417967046,
    -1.7701307697799304,
    0.9461746957575601,
    -0.6690465435572892,
    0.10578554691520431,
    -0.6690465435572892,
    0.47308734787878004,
    -1.7701307697799304,
    0.6258357354491761,
  );

  var result = c0 * dc_coeff;
  if (degree == 0u) {
    return result;
  }

  let x = dir.x;
  let y = dir.y;
  let z = dir.z;

  result = result
    - c1 * y * sh_rest_coeff(idx, channel, 0u, degree)
    + c1 * z * sh_rest_coeff(idx, channel, 1u, degree)
    - c1 * x * sh_rest_coeff(idx, channel, 2u, degree);
  if (degree == 1u) {
    return result;
  }

  let xx = x * x;
  let yy = y * y;
  let zz = z * z;
  let xy = x * y;
  let yz = y * z;
  let xz = x * z;

  result = result
    + c2[0] * xy * sh_rest_coeff(idx, channel, 3u, degree)
    + c2[1] * yz * sh_rest_coeff(idx, channel, 4u, degree)
    + c2[2] * (2.0 * zz - xx - yy) * sh_rest_coeff(idx, channel, 5u, degree)
    + c2[3] * xz * sh_rest_coeff(idx, channel, 6u, degree)
    + c2[4] * (xx - yy) * sh_rest_coeff(idx, channel, 7u, degree);
  if (degree == 2u) {
    return result;
  }

  result = result
    + c3[0] * y * (3.0 * xx - yy) * sh_rest_coeff(idx, channel, 8u, degree)
    + c3[1] * xy * z * sh_rest_coeff(idx, channel, 9u, degree)
    + c3[2] * y * (4.0 * zz - xx - yy) * sh_rest_coeff(idx, channel, 10u, degree)
    + c3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy) * sh_rest_coeff(idx, channel, 11u, degree)
    + c3[4] * x * (4.0 * zz - xx - yy) * sh_rest_coeff(idx, channel, 12u, degree)
    + c3[5] * z * (xx - yy) * sh_rest_coeff(idx, channel, 13u, degree)
    + c3[6] * x * (xx - 3.0 * yy) * sh_rest_coeff(idx, channel, 14u, degree);
  if (degree == 3u) {
    return result;
  }

  result = result
    + c4[0] * xy * (xx - yy) * sh_rest_coeff(idx, channel, 15u, degree)
    + c4[1] * yz * (3.0 * xx - yy) * sh_rest_coeff(idx, channel, 16u, degree)
    + c4[2] * xy * (7.0 * zz - 1.0) * sh_rest_coeff(idx, channel, 17u, degree)
    + c4[3] * yz * (7.0 * zz - 3.0) * sh_rest_coeff(idx, channel, 18u, degree)
    + c4[4] * (zz * (35.0 * zz - 30.0) + 3.0) * sh_rest_coeff(idx, channel, 19u, degree)
    + c4[5] * xz * (7.0 * zz - 3.0) * sh_rest_coeff(idx, channel, 20u, degree)
    + c4[6] * (xx - yy) * (7.0 * zz - 1.0) * sh_rest_coeff(idx, channel, 21u, degree)
    + c4[7] * xz * (xx - 3.0 * yy) * sh_rest_coeff(idx, channel, 22u, degree)
    + c4[8] * (xx * (xx - 3.0 * yy) - yy * (3.0 * xx - yy)) * sh_rest_coeff(idx, channel, 23u, degree);

  return result;
}

fn write_zero(slot: u32) {
  // Keep culled entries safely outside clip space to avoid degenerate triangle artifacts.
  instances[slot].center_and_axis_u = vec4<f32>(2.0, 2.0, 0.0, 0.0);
  instances[slot].axis_v_and_pad = vec4<f32>(0.0);
  instances[slot].color = vec4<f32>(0.0);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let slot = gid.x;
  if (slot >= params.len) {
    return;
  }

  let idx = sorted_indices[slot];
  let scene = scene_elems[idx];
  let pos_world = scene.position.xyz;
  let rel = pos_world - params.camera_pos.xyz;
  let p_cam = quat_rotate(params.camera_inv_q, rel);
  if (p_cam.z < params.near_plane ||
      p_cam.z > params.far_plane ||
      p_cam.z <= 1e-6) {
    write_zero(slot);
    return;
  }

  let f = 1.0 / tan(params.vertical_fov_radians * 0.5);
  let inv_z = 1.0 / p_cam.z;
  let x_ndc = (p_cam.x * f) * inv_z / params.aspect;
  let y_ndc = (p_cam.y * f) * inv_z;

  let view_rot = Mat3Rows(
    params.view_rot_row0.xyz,
    params.view_rot_row1.xyz,
    params.view_rot_row2.xyz,
  );
  let world_cov = Mat3Rows(
    scene.cov_row0.xyz,
    scene.cov_row1.xyz,
    scene.cov_row2.xyz,
  );
  let cam_cov = mat3_mul_rows(mat3_mul_rows(view_rot, world_cov), transpose_rows(view_rot));

  let tan_half_fovy = tan(params.vertical_fov_radians * 0.5);
  let tan_half_fovx = tan_half_fovy * params.aspect;
  let lim_x = 1.3 * tan_half_fovx;
  let lim_y = 1.3 * tan_half_fovy;
  let x_clamped = clamp(p_cam.x / p_cam.z, -lim_x, lim_x) * p_cam.z;
  let y_clamped = clamp(p_cam.y / p_cam.z, -lim_y, lim_y) * p_cam.z;

  let fx = f / params.aspect;
  let fy = f;
  let inv_z2 = inv_z * inv_z;
  let j0 = vec3<f32>(fx * inv_z, 0.0, -fx * x_clamped * inv_z2);
  let j1 = vec3<f32>(0.0, fy * inv_z, -fy * y_clamped * inv_z2);

  let j0m = row_mul_mat(j0, cam_cov);
  let j1m = row_mul_mat(j1, cam_cov);
  var a = dot(j0m, j0);
  var b = dot(j0m, j1);
  var c = dot(j1m, j1);

  let blur_pixels = 0.3;
  let px_ndc_x = 2.0 / max(f32(params.width), 1.0);
  let px_ndc_y = 2.0 / max(f32(params.height), 1.0);
  a = a + pow(blur_pixels * px_ndc_x, 2.0);
  c = c + pow(blur_pixels * px_ndc_y, 2.0);

  let apco2 = (a + c) * 0.5;
  let amco2 = (a - c) * 0.5;
  let term = sqrt(max(amco2 * amco2 + b * b, 0.0));
  let major = max(apco2 + term, 1e-10);
  let minor = max(apco2 - term, 1e-10);

  var axis_u_dir = vec2<f32>(1.0, 0.0);
  if (abs(b) > 1e-8) {
    axis_u_dir = normalize2_or_default(vec2<f32>(b, major - a), vec2<f32>(1.0, 0.0));
  } else if (a < c) {
    axis_u_dir = vec2<f32>(0.0, 1.0);
  }
  let axis_v_dir = vec2<f32>(-axis_u_dir.y, axis_u_dir.x);

  var major_radius = clamp(sqrt(major) * 3.0, 1e-4, 2.0);
  let minor_radius = clamp(sqrt(minor) * 3.0, 1e-4, 2.0);
  let max_anisotropy = 64.0;
  major_radius = min(major_radius, minor_radius * max_anisotropy);
  if (minor_radius < 0.001 && major_radius > 0.2) {
    write_zero(slot);
    return;
  }
  let axis_u = axis_u_dir * major_radius;
  let axis_v = axis_v_dir * minor_radius;

  let extent_x = abs(axis_u.x) + abs(axis_v.x);
  let extent_y = abs(axis_u.y) + abs(axis_v.y);
  if (x_ndc + extent_x < -1.0 ||
      x_ndc - extent_x >  1.0 ||
      y_ndc + extent_y < -1.0 ||
      y_ndc - extent_y >  1.0) {
    write_zero(slot);
    return;
  }

  let alpha = clamp(1.0 / (1.0 + exp(-scene.opacity_and_pad.x)), 0.0, 1.0);
  let degree = min(params.sh_degree, 4u);
  let dir_world = normalize3_or_default(rel, vec3<f32>(0.0, 0.0, 1.0));
  let sh_rgb = vec3<f32>(
    eval_sh_channel(idx, 0u, scene.color_dc.x, dir_world, degree),
    eval_sh_channel(idx, 1u, scene.color_dc.y, dir_world, degree),
    eval_sh_channel(idx, 2u, scene.color_dc.z, dir_world, degree),
  );
  let rgb = clamp(sh_rgb + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));

  instances[slot].center_and_axis_u = vec4<f32>(x_ndc, y_ndc, axis_u.x, axis_u.y);
  instances[slot].axis_v_and_pad = vec4<f32>(axis_v.x, axis_v.y, 0.0, 0.0);
  instances[slot].color = vec4<f32>(rgb * alpha, alpha);
}
