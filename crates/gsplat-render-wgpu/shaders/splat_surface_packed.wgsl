// Packed active-atlas SortedAlpha path (Phase B, no streaming).
// Hot records are a tightly packed storage buffer (5 x u32 = 20 bytes/splat):
// position/opacity, scale/flags, rotation, color. World covariance is rebuilt
// per-vertex from log-encoded scale plus a smallest-three-packed quaternion
// rather than carried precomputed in the hot record.
// View-dependent color is written into the hot color word by a CPU color-refresh
// pass before draw.

struct Params {
  camera_pos: vec4<f32>,
  view_rot_row0: vec4<f32>,
  view_rot_row1: vec4<f32>,
  view_rot_row2: vec4<f32>,
  bounds_min: vec4<f32>,
  bounds_extent: vec4<f32>,
  sh_scales: vec4<f32>,
  log_scale_min: f32,
  log_scale_extent: f32,
  vertical_fov_radians: f32,
  near_plane: f32,
  far_plane: f32,
  aspect: f32,
  width: u32,
  height: u32,
  sh_degree: u32,
  len: u32,
  atlas_width: u32,
  _pad0: u32,
};

struct CovTerms {
  xx: f32,
  xy: f32,
  xz: f32,
  yy: f32,
  yz: f32,
  zz: f32,
};

struct ProjectedSplat {
  center: vec2<f32>,
  axis_u: vec2<f32>,
  axis_v: vec2<f32>,
  alpha: f32,
};

struct UnpackedSource {
  position: vec3<f32>,
  alpha: f32,
  covariance: CovTerms,
  color_rgb: vec3<f32>,
};

@group(0) @binding(0)
var<storage, read> sorted_indices: array<u32>;
@group(0) @binding(1)
var<storage, read> hot_records: array<u32>;
@group(0) @binding(2)
var<uniform> params: Params;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) local: vec2<f32>,
};

fn quad_offset(vertex_index: u32) -> vec2<f32> {
  let offsets = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0,  1.0),
  );
  return offsets[vertex_index];
}

fn alpha_extent_scale(alpha: f32) -> f32 {
  let s2 = log(max(alpha, 1e-12) * 256.0) / 4.5;
  return sqrt(clamp(s2, 0.0, 1.0));
}

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

fn invalid_splat() -> ProjectedSplat {
  return ProjectedSplat(
    vec2<f32>(2.0, 2.0),
    vec2<f32>(0.0, 0.0),
    vec2<f32>(0.0, 0.0),
    0.0,
  );
}

fn decode_position(encoded: vec3<u32>) -> vec3<f32> {
  let t = vec3<f32>(encoded) / 65535.0;
  return params.bounds_min.xyz + t * params.bounds_extent.xyz;
}

fn decode_opacity(encoded: u32) -> f32 {
  let alpha = clamp(f32(encoded & 0xffu) / 255.0, 1e-4, 1.0 - 1e-4);
  return alpha;
}

fn unpack_color_rgb10(packed: u32) -> vec3<f32> {
  return vec3<f32>(
    f32(packed & 0x3ffu),
    f32((packed >> 10u) & 0x3ffu),
    f32((packed >> 20u) & 0x3ffu),
  ) / 1023.0;
}

fn decode_log_scale(encoded: u32) -> f32 {
  return params.log_scale_min + (f32(encoded) / 255.0) * params.log_scale_extent;
}

// Smallest-three quaternion decode (10+10+10 bits + 2-bit dropped-component
// index), mirroring `pack_quat_smallest_three` on the CPU side.
fn unpack_quat_smallest_three(bits: u32) -> vec4<f32> {
  let max_index = bits & 3u;
  let inv_sqrt2 = 0.70710678118654752440;
  let bits0 = (bits >> 2u) & 0x3ffu;
  let bits1 = (bits >> 12u) & 0x3ffu;
  let bits2 = (bits >> 22u) & 0x3ffu;
  let c0 = (f32(bits0) / 1023.0 * 2.0 - 1.0) * inv_sqrt2;
  let c1 = (f32(bits1) / 1023.0 * 2.0 - 1.0) * inv_sqrt2;
  let c2 = (f32(bits2) / 1023.0 * 2.0 - 1.0) * inv_sqrt2;
  let sum_sq = c0 * c0 + c1 * c1 + c2 * c2;
  let dropped = sqrt(max(1.0 - sum_sq, 0.0));
  var q = vec4<f32>(c0, c1, c2, dropped);
  if (max_index == 0u) {
    q = vec4<f32>(dropped, c0, c1, c2);
  } else if (max_index == 1u) {
    q = vec4<f32>(c0, dropped, c1, c2);
  } else if (max_index == 2u) {
    q = vec4<f32>(c0, c1, dropped, c2);
  }
  return normalize(q);
}

// Row-major rotation matrix stored as three column entries of a mat3x3; each
// `rot[i]` is row `i` of the rotation matrix (see `world_covariance_from_scale_rotation`).
fn quat_to_mat3(q: vec4<f32>) -> mat3x3<f32> {
  let x = q.x;
  let y = q.y;
  let z = q.z;
  let w = q.w;
  let x2 = x + x;
  let y2 = y + y;
  let z2 = z + z;
  let xx = x * x2;
  let yy = y * y2;
  let zz = z * z2;
  let xy = x * y2;
  let xz = x * z2;
  let yz = y * z2;
  let wx = w * x2;
  let wy = w * y2;
  let wz = w * z2;
  // Matches the direct path's `quat_to_mat3` sign convention exactly so the
  // rebuilt covariance is not the transpose (inverse rotation) of the
  // reference orientation.
  let row0 = vec3<f32>(1.0 - (yy + zz), xy - wz, xz + wy);
  let row1 = vec3<f32>(xy + wz, 1.0 - (xx + zz), yz - wx);
  let row2 = vec3<f32>(xz - wy, yz + wx, 1.0 - (xx + yy));
  return mat3x3<f32>(row0, row1, row2);
}

// Rebuilds world-space covariance terms directly from log-encoded scale and a
// packed rotation quaternion (no precomputed covariance in the hot record).
fn world_covariance_from_scale_rotation(scale_log: vec3<f32>, quat: vec4<f32>) -> CovTerms {
  let sx2 = exp(2.0 * scale_log.x);
  let sy2 = exp(2.0 * scale_log.y);
  let sz2 = exp(2.0 * scale_log.z);
  let rot = quat_to_mat3(normalize(quat));
  let r0 = rot[0];
  let r1 = rot[1];
  let r2 = rot[2];
  return CovTerms(
    sx2 * r0.x * r0.x + sy2 * r0.y * r0.y + sz2 * r0.z * r0.z,
    sx2 * r0.x * r1.x + sy2 * r0.y * r1.y + sz2 * r0.z * r1.z,
    sx2 * r0.x * r2.x + sy2 * r0.y * r2.y + sz2 * r0.z * r2.z,
    sx2 * r1.x * r1.x + sy2 * r1.y * r1.y + sz2 * r1.z * r1.z,
    sx2 * r1.x * r2.x + sy2 * r1.y * r2.y + sz2 * r1.z * r2.z,
    sx2 * r2.x * r2.x + sy2 * r2.y * r2.y + sz2 * r2.z * r2.z,
  );
}

fn load_source(slot: u32) -> UnpackedSource {
  let base = slot * 5u;
  let po0 = hot_records[base];
  let po1 = hot_records[base + 1u];
  let scale_flags = hot_records[base + 2u];
  let rotation_bits = hot_records[base + 3u];
  let color = hot_records[base + 4u];
  let position = decode_position(vec3<u32>(
    po0 & 0xffffu,
    po0 >> 16u,
    po1 & 0xffffu,
  ));
  let alpha = decode_opacity(po1 >> 16u);
  let scale_log = vec3<f32>(
    decode_log_scale(scale_flags & 0xffu),
    decode_log_scale((scale_flags >> 8u) & 0xffu),
    decode_log_scale((scale_flags >> 16u) & 0xffu),
  );
  let quat = unpack_quat_smallest_three(rotation_bits);
  let covariance = world_covariance_from_scale_rotation(scale_log, quat);
  let color_rgb = unpack_color_rgb10(color);
  return UnpackedSource(position, alpha, covariance, color_rgb);
}

fn project_splat(source: UnpackedSource) -> ProjectedSplat {
  let rel = source.position - params.camera_pos.xyz;
  let r0 = params.view_rot_row0.xyz;
  let r1 = params.view_rot_row1.xyz;
  let r2 = params.view_rot_row2.xyz;
  let p_cam = vec3<f32>(dot(r0, rel), dot(r1, rel), dot(r2, rel));
  if (p_cam.z < params.near_plane ||
      p_cam.z > params.far_plane ||
      p_cam.z <= 1e-6) {
    return invalid_splat();
  }

  let f = 1.0 / tan(params.vertical_fov_radians * 0.5);
  let inv_z = 1.0 / p_cam.z;
  let x_ndc = (p_cam.x * f) * inv_z / params.aspect;
  let y_ndc = (p_cam.y * f) * inv_z;

  let cov_cam = transform_covariance_terms_to_camera(source.covariance, r0, r1, r2);

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
    return invalid_splat();
  }

  return ProjectedSplat(vec2<f32>(x_ndc, y_ndc), axis_u, axis_v, source.alpha);
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let idx = sorted_indices[instance_index];
  let source = load_source(idx);
  let projected = project_splat(source);
  let local = quad_offset(vertex_index) * alpha_extent_scale(projected.alpha);
  let offset = projected.axis_u * local.x + projected.axis_v * local.y;

  var out: VsOut;
  out.position = vec4<f32>(projected.center + offset, 0.0, 1.0);
  out.color = vec4<f32>(source.color_rgb * projected.alpha, projected.alpha);
  out.local = local;
  return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
  let r2 = dot(input.local, input.local);
  if (r2 > 1.0) {
    discard;
  }

  let g = exp(-4.5 * r2);
  let alpha = input.color.a * g;
  if (alpha <= (1.0 / 256.0)) {
    discard;
  }
  return vec4<f32>(input.color.rgb * g, alpha);
}
