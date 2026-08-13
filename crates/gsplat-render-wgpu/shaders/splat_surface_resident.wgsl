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
  order_stride_words: u32,
  order_id_offset_words: u32,
  _order_pad0: u32,
  _order_pad1: u32,
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

@group(0) @binding(0)
var<storage, read> sorted_index_words: array<u32>;
@group(0) @binding(1)
var<storage, read> source_elems: array<SurfaceSourceElem>;
@group(0) @binding(2)
var<storage, read> sh_rest: array<f32>;
@group(0) @binding(3)
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

// fs_main discards fragments with alpha * exp(-4.5 * r2) <= 1/256, so the
// quad only needs to cover r2 <= ln(256 * alpha) / 4.5. Scaling the offset and
// the local coordinate by the same factor keeps the pixel -> gaussian mapping
// identical while skipping rasterization of fragments that would be discarded.
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

fn normalize3_or_default(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
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

fn project_splat(source: SurfaceSourceElem) -> ProjectedSplat {
  let rel = source.position.xyz - params.camera_pos.xyz;
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
    return invalid_splat();
  }

  return ProjectedSplat(vec2<f32>(x_ndc, y_ndc), axis_u, axis_v, source.covariance1.z);
}

// Reads one SH coefficient for all three color channels. The buffer layout is
// channel-major per splat, so the three channel values for coefficient
// `coeff` sit `per_channel` floats apart.
fn sh_rest_vec3(base: u32, per_channel: u32, coeff: u32) -> vec3<f32> {
  return vec3<f32>(
    sh_rest[base + coeff],
    sh_rest[base + per_channel + coeff],
    sh_rest[base + 2u * per_channel + coeff],
  );
}

// Evaluates all three channels together so the direction basis polynomials
// are computed once per splat instead of once per channel. The per-channel
// accumulation order matches the previous scalar evaluation exactly.
fn eval_sh_rgb(idx: u32, dc: vec3<f32>, dir: vec3<f32>, degree: u32) -> vec3<f32> {
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

  var result = c0 * dc;
  if (degree == 0u) {
    return result;
  }

  let coeff_total = (degree + 1u) * (degree + 1u);
  let per_channel = coeff_total - 1u;
  let base = idx * per_channel * 3u;

  let x = dir.x;
  let y = dir.y;
  let z = dir.z;

  result = result
    - c1 * y * sh_rest_vec3(base, per_channel, 0u)
    + c1 * z * sh_rest_vec3(base, per_channel, 1u)
    - c1 * x * sh_rest_vec3(base, per_channel, 2u);
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
    + c2[0] * xy * sh_rest_vec3(base, per_channel, 3u)
    + c2[1] * yz * sh_rest_vec3(base, per_channel, 4u)
    + c2[2] * (2.0 * zz - xx - yy) * sh_rest_vec3(base, per_channel, 5u)
    + c2[3] * xz * sh_rest_vec3(base, per_channel, 6u)
    + c2[4] * (xx - yy) * sh_rest_vec3(base, per_channel, 7u);
  if (degree == 2u) {
    return result;
  }

  result = result
    + c3[0] * y * (3.0 * xx - yy) * sh_rest_vec3(base, per_channel, 8u)
    + c3[1] * xy * z * sh_rest_vec3(base, per_channel, 9u)
    + c3[2] * y * (4.0 * zz - xx - yy) * sh_rest_vec3(base, per_channel, 10u)
    + c3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy) * sh_rest_vec3(base, per_channel, 11u)
    + c3[4] * x * (4.0 * zz - xx - yy) * sh_rest_vec3(base, per_channel, 12u)
    + c3[5] * z * (xx - yy) * sh_rest_vec3(base, per_channel, 13u)
    + c3[6] * x * (xx - 3.0 * yy) * sh_rest_vec3(base, per_channel, 14u);
  if (degree == 3u) {
    return result;
  }

  result = result
    + c4[0] * xy * (xx - yy) * sh_rest_vec3(base, per_channel, 15u)
    + c4[1] * yz * (3.0 * xx - yy) * sh_rest_vec3(base, per_channel, 16u)
    + c4[2] * xy * (7.0 * zz - 1.0) * sh_rest_vec3(base, per_channel, 17u)
    + c4[3] * yz * (7.0 * zz - 3.0) * sh_rest_vec3(base, per_channel, 18u)
    + c4[4] * (zz * (35.0 * zz - 30.0) + 3.0) * sh_rest_vec3(base, per_channel, 19u)
    + c4[5] * xz * (7.0 * zz - 3.0) * sh_rest_vec3(base, per_channel, 20u)
    + c4[6] * (xx - yy) * (7.0 * zz - 1.0) * sh_rest_vec3(base, per_channel, 21u)
    + c4[7] * xz * (xx - 3.0 * yy) * sh_rest_vec3(base, per_channel, 22u)
    + c4[8] * (xx * (xx - 3.0 * yy) - yy * (3.0 * xx - yy)) * sh_rest_vec3(base, per_channel, 23u);

  return result;
}

fn eval_color(idx: u32, source: SurfaceSourceElem, alpha: f32) -> vec4<f32> {
  let dir = normalize3_or_default(source.position.xyz - params.camera_pos.xyz, vec3<f32>(0.0, 0.0, 1.0));
  let degree = min(params.sh_degree, 4u);
  let sh_rgb = eval_sh_rgb(idx, source.color_dc.xyz, dir, degree);
  let rgb = clamp(sh_rgb + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
  return vec4<f32>(rgb * alpha, alpha);
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let order_word = instance_index * params.order_stride_words
    + params.order_id_offset_words;
  let idx = sorted_index_words[order_word];
  let source = source_elems[idx];
  let projected = project_splat(source);
  let local = quad_offset(vertex_index) * alpha_extent_scale(projected.alpha);
  let offset = projected.axis_u * local.x + projected.axis_v * local.y;

  var out: VsOut;
  out.position = vec4<f32>(projected.center + offset, 0.0, 1.0);
  out.color = eval_color(idx, source, projected.alpha);
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
