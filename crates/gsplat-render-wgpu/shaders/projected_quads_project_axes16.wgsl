// Candidate binary16-axis rank-indexed projection cache for Resident SortedAlpha quads.

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

struct DrawIndirectArgs {
  vertex_count: u32,
  instance_count: atomic<u32>,
  first_vertex: u32,
  // Internal projection-mode flag. GPU-order and compacted CPU paths keep it
  // zero; the no-scan downlevel CPU path sets it to one so telemetry can still
  // obtain C. It is never consumed as an actual indirect draw in that path.
  first_instance: u32,
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
var<storage, read> order_words: array<u32>;
@group(0) @binding(1)
var<storage, read> position_alpha: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> covariance0: array<vec4<f32>>;
@group(0) @binding(3)
var<storage, read> covariance1: array<vec2<f32>>;
@group(0) @binding(4)
var<uniform> params: Params;
// Centers/source IDs remain 16 bytes per visible rank; only axes use the
// candidate 8-byte packed record. Projection and contribution math stay f32.
@group(0) @binding(5)
var<storage, read_write> projected_center_source: array<vec4<f32>>;
@group(0) @binding(6)
var<storage, read_write> projected_axes: array<vec2<u32>>;
@group(0) @binding(7)
var<storage, read_write> draw_args: DrawIndirectArgs;
@group(0) @binding(8)
var<storage, read_write> contributor_group_counts: array<atomic<u32>>;

const WORKGROUP_SIZE: u32 = 128u;
const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;

var<workgroup> contributor_count: atomic<u32>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
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

fn invalid_splat() -> ProjectedSplat {
  return ProjectedSplat(
    vec2<f32>(2.0, 2.0),
    vec2<f32>(0.0, 0.0),
    vec2<f32>(0.0, 0.0),
    0.0,
  );
}

fn next_up(value: f32) -> f32 {
  let bits = bitcast<u32>(value);
  let magnitude = bits & 0x7fffffffu;
  if (magnitude > 0x7f800000u || bits == 0x7f800000u) {
    return value;
  }
  if (magnitude == 0u) {
    return bitcast<f32>(1u);
  }
  return bitcast<f32>(select(bits - 1u, bits + 1u, value > 0.0));
}

fn next_down(value: f32) -> f32 {
  let bits = bitcast<u32>(value);
  let magnitude = bits & 0x7fffffffu;
  if (magnitude > 0x7f800000u || bits == 0xff800000u) {
    return value;
  }
  if (magnitude == 0u) {
    return bitcast<f32>(0x80000001u);
  }
  return bitcast<f32>(select(bits + 1u, bits - 1u, value > 0.0));
}

fn is_finite(value: f32) -> bool {
  return (bitcast<u32>(value) & 0x7f800000u) != 0x7f800000u;
}

// Reject only when an outward-rounded envelope of the *unscaled* 3-sigma quad
// is strictly outside one clip plane. The vertex shader's alpha support scale
// is in [0, 1], so its actual quad is a subset of this envelope. This avoids
// relying on log/sqrt producing bit-identical approximations in compute and
// vertex stages. Every arithmetic step is expanded outwards; overflow and any
// non-finite input fail open and retain the splat.
fn full_quad_is_strictly_outside_clip(
  center: vec2<f32>,
  axis_u: vec2<f32>,
  axis_v: vec2<f32>,
  alpha: f32,
) -> bool {
  if (!is_finite(center.x) || !is_finite(center.y)
      || !is_finite(axis_u.x) || !is_finite(axis_u.y)
      || !is_finite(axis_v.x) || !is_finite(axis_v.y)
      || !is_finite(alpha)) {
    return false;
  }
  let extent = vec2<f32>(
    next_up(abs(axis_u.x) + abs(axis_v.x)),
    next_up(abs(axis_u.y) + abs(axis_v.y)),
  );
  let conservative_minimum = vec2<f32>(
    next_down(center.x - extent.x),
    next_down(center.y - extent.y),
  );
  let conservative_maximum = vec2<f32>(
    next_up(center.x + extent.x),
    next_up(center.y + extent.y),
  );
  return conservative_maximum.x < -1.0
    || conservative_minimum.x > 1.0
    || conservative_maximum.y < -1.0
    || conservative_minimum.y > 1.0;
}

// This is deliberately the same projection contract as
// splat_surface_resident.wgsl. Moving it from four vertex invocations to one
// compute invocation must not change the image contract.
fn project_splat(source_id: u32) -> ProjectedSplat {
  let pa = position_alpha[source_id];
  let cov0 = covariance0[source_id];
  let cov1 = covariance1[source_id];
  let rel = pa.xyz - params.camera_pos.xyz;
  let r0 = params.view_rot_row0.xyz;
  let r1 = params.view_rot_row1.xyz;
  let r2 = params.view_rot_row2.xyz;
  let p_cam = vec3<f32>(
    canonical_dot3(r0, rel),
    canonical_dot3(r1, rel),
    canonical_dot3(r2, rel),
  );
  if (p_cam.z < params.near_plane || p_cam.z > params.far_plane ||
      pa.w < ALPHA_THRESHOLD) {
    return invalid_splat();
  }

  let f = 1.0 / tan(params.vertical_fov_radians * 0.5);
  let inv_z = 1.0 / p_cam.z;
  let x_ndc = (p_cam.x * f) * inv_z / params.aspect;
  let y_ndc = (p_cam.y * f) * inv_z;
  let covariance = CovTerms(cov0.x, cov0.y, cov0.z, cov0.w, cov1.x, cov1.y);
  let cov_cam = transform_covariance_terms_to_camera(covariance, r0, r1, r2);
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
  if (full_quad_is_strictly_outside_clip(
      vec2<f32>(x_ndc, y_ndc), axis_u, axis_v, pa.w)) {
    return invalid_splat();
  }
  return ProjectedSplat(
    vec2<f32>(x_ndc, y_ndc),
    axis_u,
    axis_v,
    pa.w,
  );
}

@compute @workgroup_size(128)
fn main(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let capacity_group_count =
    (params.len + WORKGROUP_SIZE - 1u) / WORKGROUP_SIZE;
  // Dispatch2d may cover a rectangular superset when the logical group count
  // exceeds max_compute_workgroups_per_dimension. This uniform early return
  // prevents the rectangle tail from addressing group-count storage.
  if (group >= capacity_group_count) {
    return;
  }
  let rank = group * WORKGROUP_SIZE + lane;
  if (lane == 0u) {
    atomicStore(&contributor_count, 0u);
  }
  workgroupBarrier();

  let visible_count = atomicLoad(&draw_args.instance_count);
  var contributes = false;
  if (rank < visible_count) {
    let order_word = rank * params.order_stride_words + params.order_id_offset_words;
    let source_id = order_words[order_word];
    let projected = project_splat(source_id);
    projected_center_source[rank] = vec4<f32>(
      projected.center,
      projected.alpha,
      bitcast<f32>(source_id),
    );
    projected_axes[rank] = vec2<u32>(pack2x16float(projected.axis_u), pack2x16float(projected.axis_v));
    // Invalid splats use alpha zero. A NaN source alpha is deliberately kept
    // fail-open here rather than being silently classified as zero work.
    contributes = !(projected.alpha < ALPHA_THRESHOLD);
  } else if (rank < params.len) {
    // GPU ordering dispatches resident capacity and guards with its separate
    // candidate args. Clear the trailing cache so a prior larger prefix can
    // never become a contributor on a later compact pass.
    projected_center_source[rank] = vec4<f32>(2.0, 2.0, 0.0, 0.0);
    projected_axes[rank] = vec2<u32>(0u);
  }

  if (contributes) {
    atomicAdd(&contributor_count, 1u);
  }
  workgroupBarrier();
  if (lane == 0u) {
    let count = atomicLoad(&contributor_count);
    atomicStore(&contributor_group_counts[group], count);
    if (draw_args.first_instance != 0u) {
      // Only downlevel direct drawing lacks the prefix scan that normally
      // writes the final sum. Keep this fallback exact without imposing one
      // globally contended atomic on production compacted workgroups.
      atomicAdd(
        &contributor_group_counts[arrayLength(&contributor_group_counts) - 1u],
        count,
      );
    }
  }
}
