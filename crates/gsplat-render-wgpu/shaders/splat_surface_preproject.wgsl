struct InstanceData {
  center_and_axis_u: vec4<f32>,
  axis_v_index_alpha: vec4<f32>,
};

struct SurfaceSourceElem {
  position: vec4<f32>,
  covariance0: vec4<f32>,
  covariance1: vec4<f32>,
  color_dc: vec4<f32>,
};

struct Params {
  camera_pos: vec4<f32>,
  view_rot_row0: vec4<f32>,
  view_rot_row1: vec4<f32>,
  view_rot_row2: vec4<f32>,
  vertical_fov_radians: f32,
  near_plane: f32,
  far_plane: f32,
  aspect: f32,
  width: u32,
  height: u32,
  sh_degree: u32,
  len: u32,
};

struct CovTerms {
  xx: f32,
  xy: f32,
  xz: f32,
  yy: f32,
  yz: f32,
  zz: f32,
};

@group(0) @binding(0)
var<storage, read> sorted_indices: array<u32>;
@group(0) @binding(1)
var<storage, read> source_elems: array<SurfaceSourceElem>;
@group(0) @binding(2)
var<uniform> params: Params;
@group(0) @binding(3)
var<storage, read_write> instances: array<InstanceData>;

fn normalize2_or_default(v: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
  let len2 = dot(v, v);
  if (len2 <= 1e-20) {
    return fallback;
  }
  return v * inverseSqrt(len2);
}

fn covariance_quadratic(c: CovTerms, r: vec3<f32>) -> f32 {
  return r.x * r.x * c.xx
    + 2.0 * r.x * r.y * c.xy
    + 2.0 * r.x * r.z * c.xz
    + r.y * r.y * c.yy
    + 2.0 * r.y * r.z * c.yz
    + r.z * r.z * c.zz;
}

fn covariance_bilinear(c: CovTerms, a: vec3<f32>, b: vec3<f32>) -> f32 {
  let bx = c.xx * b.x + c.xy * b.y + c.xz * b.z;
  let by = c.xy * b.x + c.yy * b.y + c.yz * b.z;
  let bz = c.xz * b.x + c.yz * b.y + c.zz * b.z;
  return a.x * bx + a.y * by + a.z * bz;
}

fn transform_covariance_terms_to_camera(c: CovTerms, r0: vec3<f32>, r1: vec3<f32>, r2: vec3<f32>) -> CovTerms {
  return CovTerms(
    covariance_quadratic(c, r0),
    covariance_bilinear(c, r0, r1),
    covariance_bilinear(c, r0, r2),
    covariance_quadratic(c, r1),
    covariance_bilinear(c, r1, r2),
    covariance_quadratic(c, r2),
  );
}

fn write_zero(slot: u32) {
  instances[slot].center_and_axis_u = vec4<f32>(2.0, 2.0, 0.0, 0.0);
  instances[slot].axis_v_index_alpha = vec4<f32>(0.0);
}

@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let slot = gid.x;
  if (slot >= params.len) {
    return;
  }

  let idx = sorted_indices[slot];
  let source = source_elems[idx];
  let rel = source.position.xyz - params.camera_pos.xyz;
  let r0 = params.view_rot_row0.xyz;
  let r1 = params.view_rot_row1.xyz;
  let r2 = params.view_rot_row2.xyz;
  let p_cam = vec3<f32>(dot(r0, rel), dot(r1, rel), dot(r2, rel));
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

  let world_cov = CovTerms(
    source.covariance0.x,
    source.covariance0.y,
    source.covariance0.z,
    source.covariance0.w,
    source.covariance1.x,
    source.covariance1.y,
  );
  let cov_cam = transform_covariance_terms_to_camera(world_cov, r0, r1, r2);

  let tan_half_fovy = tan(params.vertical_fov_radians * 0.5);
  let tan_half_fovx = tan_half_fovy * params.aspect;
  let lim_x = 1.3 * tan_half_fovx;
  let lim_y = 1.3 * tan_half_fovy;
  let x_clamped = clamp(p_cam.x / p_cam.z, -lim_x, lim_x) * p_cam.z;
  let y_clamped = clamp(p_cam.y / p_cam.z, -lim_y, lim_y) * p_cam.z;

  let fx = f / params.aspect;
  let fy = f;
  let inv_z2 = inv_z * inv_z;
  let j00 = fx * inv_z;
  let j02 = -fx * x_clamped * inv_z2;
  let j11 = fy * inv_z;
  let j12 = -fy * y_clamped * inv_z2;

  let cov01 = j00 * j11 * cov_cam.xy
    + j00 * j12 * cov_cam.xz
    + j02 * j11 * cov_cam.yz
    + j02 * j12 * cov_cam.zz;
  var a = j00 * j00 * cov_cam.xx + 2.0 * j00 * j02 * cov_cam.xz + j02 * j02 * cov_cam.zz;
  let b = cov01;
  var c = j11 * j11 * cov_cam.yy + 2.0 * j11 * j12 * cov_cam.yz + j12 * j12 * cov_cam.zz;

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
  major_radius = min(major_radius, minor_radius * 64.0);
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

  instances[slot].center_and_axis_u = vec4<f32>(x_ndc, y_ndc, axis_u.x, axis_u.y);
  instances[slot].axis_v_index_alpha = vec4<f32>(axis_v.x, axis_v.y, f32(idx), source.covariance1.z);
}
