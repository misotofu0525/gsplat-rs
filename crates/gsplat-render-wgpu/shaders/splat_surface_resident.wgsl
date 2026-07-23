// Exact-count compact resident SortedAlpha draw path.

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
  order_stride_words: u32,
  order_id_offset_words: u32,
  source_position_stride_words: u32,
  source_position_offset_words: u32,
};

struct CovTerms {
  xx: f32,
  xy: f32,
  xz: f32,
  yy: f32,
  yz: f32,
  zz: f32,
};

struct Source {
  position: vec3<f32>,
  alpha: f32,
  covariance: CovTerms,
  color_rgb: vec3<f32>,
};

struct ProjectedSplat {
  center: vec2<f32>,
  axis_u: vec2<f32>,
  axis_v: vec2<f32>,
  alpha: f32,
};

@group(0) @binding(0)
var<storage, read> order_words: array<u32>;
@group(0) @binding(1)
var<storage, read> position_alpha: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> covariance0: array<vec4<f32>>;
@group(0) @binding(3)
var<storage, read> covariance1: array<vec2<f32>>;
@group(0) @binding(4)
var<storage, read> resolved_color: array<vec2<u32>>;
@group(0) @binding(5)
var<uniform> params: Params;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) local: vec2<f32>,
};

fn quad_offset(vertex_index: u32) -> vec2<f32> {
  // Canonical four-vertex triangle strip: BL, BR, TL, TR.
  let offsets = array<vec2<f32>, 4>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 1.0,  1.0),
  );
  return offsets[vertex_index];
}

// Conservatively bound rasterization by the 1/256 opacity iso-contour. The
// fragment threshold is 1/255, so every potentially contributing sample stays
// covered while guaranteed-discard work is removed without changing the draw
// instance count or pixel-to-Gaussian mapping.
fn alpha_extent_scale(alpha: f32) -> f32 {
  return sqrt(clamp(log(max(alpha, 1e-12) * 256.0) / 4.5, 0.0, 1.0));
}

fn canonical_dot3(left: vec3<f32>, right: vec3<f32>) -> f32 {
  let xy = fma(left.y, right.y, left.x * right.x);
  return fma(left.z, right.z, xy);
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

fn unpack_color_rgb18e8(bits: vec2<u32>) -> vec3<f32> {
  let exponent_code = (bits.y >> 22u) & 0xffu;
  if (exponent_code == 0u) {
    return vec3<f32>(0.0);
  }
  let r = bits.x & 0x3ffffu;
  let g = (bits.x >> 18u) | ((bits.y & 0xfu) << 14u);
  let b = (bits.y >> 4u) & 0x3ffffu;
  let scale = exp2(f32(i32(exponent_code) - 127));
  return vec3<f32>(f32(r), f32(g), f32(b)) * (scale / 262143.0);
}

fn load_source(slot: u32) -> Source {
  let pa = position_alpha[slot];
  let cov0 = covariance0[slot];
  let cov1 = covariance1[slot];
  return Source(
    pa.xyz,
    pa.w,
    CovTerms(cov0.x, cov0.y, cov0.z, cov0.w, cov1.x, cov1.y),
    unpack_color_rgb18e8(resolved_color[slot]),
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

fn project_splat(source: Source) -> ProjectedSplat {
  let rel = source.position - params.camera_pos.xyz;
  let r0 = params.view_rot_row0.xyz;
  let r1 = params.view_rot_row1.xyz;
  let r2 = params.view_rot_row2.xyz;
  let p_cam = vec3<f32>(
    canonical_dot3(r0, rel),
    canonical_dot3(r1, rel),
    canonical_dot3(r2, rel),
  );
  if (p_cam.z < params.near_plane || p_cam.z > params.far_plane) {
    return invalid_splat();
  }

  let f = 1.0 / tan(params.vertical_fov_radians * 0.5);
  let inv_z = 1.0 / p_cam.z;
  let x_ndc = (p_cam.x * f) * inv_z / params.aspect;
  let y_ndc = (p_cam.y * f) * inv_z;
  let cov_cam = transform_covariance_terms_to_camera(source.covariance, r0, r1, r2);
  let tan_half_fovy = tan(params.vertical_fov_radians * 0.5);
  let tan_half_fovx = tan_half_fovy * params.aspect;
  let x_clamped = clamp(p_cam.x / p_cam.z, -1.3 * tan_half_fovx, 1.3 * tan_half_fovx) * p_cam.z;
  let y_clamped = clamp(p_cam.y / p_cam.z, -1.3 * tan_half_fovy, 1.3 * tan_half_fovy) * p_cam.z;
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
  let px_ndc_x = 2.0 / max(f32(params.width), 1.0);
  let px_ndc_y = 2.0 / max(f32(params.height), 1.0);
  a = a + 0.3 * px_ndc_x * px_ndc_x;
  c = c + 0.3 * px_ndc_y * px_ndc_y;
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
  let major_radius = max(sqrt(major) * 3.0, 1e-4);
  let minor_radius = max(sqrt(minor) * 3.0, 1e-4);
  let axis_u = axis_u_dir * major_radius;
  let axis_v = axis_v_dir * minor_radius;
  let extent_x = abs(axis_u.x) + abs(axis_v.x);
  let extent_y = abs(axis_u.y) + abs(axis_v.y);
  if (x_ndc + extent_x < -1.0 || x_ndc - extent_x > 1.0 ||
      y_ndc + extent_y < -1.0 || y_ndc - extent_y > 1.0) {
    return invalid_splat();
  }
  return ProjectedSplat(vec2<f32>(x_ndc, y_ndc), axis_u, axis_v, source.alpha);
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let order_word = instance_index * params.order_stride_words + params.order_id_offset_words;
  let slot = order_words[order_word];
  let source = load_source(slot);
  let projected = project_splat(source);
  var out: VsOut;
  let local = quad_offset(vertex_index) * alpha_extent_scale(projected.alpha);
  let offset = projected.axis_u * local.x + projected.axis_v * local.y;
  out.position = vec4<f32>(projected.center + offset, 0.0, 1.0);
  out.color = vec4<f32>(source.color_rgb, projected.alpha);
  out.local = local;
  return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
  let r2 = dot(input.local, input.local);
  let g = exp(-4.5 * r2);
  let alpha = min(0.99, input.color.a * g);
  if (alpha < (1.0 / 255.0)) {
    discard;
  }
  return vec4<f32>(input.color.rgb * alpha, alpha);
}
