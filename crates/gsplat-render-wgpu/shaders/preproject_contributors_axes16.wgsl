// Candidate binary16-axis source-indexed S -> V -> C projection and deterministic source-order
// compaction input for the Phase 2 resident GPU order path. V is the
// near/far/alpha candidate population; C additionally survives full-quad clip.

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

struct OrderControl {
  contributor_count: u32,
  active_group_count: u32,
  dispatch_x: u32,
  dispatch_y: u32,
  dispatch_z: u32,
  dispatch_limit: u32,
  capacity_count: u32,
  _pad0: u32,
};

struct DrawIndirectArgs {
  vertex_count: u32,
  instance_count: atomic<u32>,
  first_vertex: u32,
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
  key: u32,
  candidate: u32,
};

@group(0) @binding(0)
var<storage, read> position_alpha: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> covariance0: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> covariance1: array<vec2<f32>>;
@group(0) @binding(3)
var<uniform> params: Params;
@group(0) @binding(4)
var<storage, read_write> projected_center_alpha_key: array<vec4<f32>>;
@group(0) @binding(5)
var<storage, read_write> projected_axes: array<vec2<u32>>;
@group(0) @binding(6)
var<storage, read_write> contributor_group_counts: array<atomic<u32>>;
@group(0) @binding(7)
var<storage, read_write> candidate_group_counts: array<atomic<u32>>;

@group(1) @binding(0)
var<storage, read> compact_center_alpha_key: array<vec4<f32>>;
@group(1) @binding(1)
var<storage, read> contributor_group_offsets: array<u32>;
@group(1) @binding(2)
var<storage, read_write> compact_keys: array<u32>;
@group(1) @binding(3)
var<storage, read_write> compact_ids: array<u32>;
@group(1) @binding(4)
var<storage, read_write> order_control: OrderControl;
@group(1) @binding(5)
var<storage, read_write> draw_args: DrawIndirectArgs;
@group(1) @binding(6)
var<uniform> compact_params: Params;

const WORKGROUP_SIZE: u32 = 128u;
const SORT_TILE_SIZE: u32 = 1024u;
const MASK_WORD_COUNT: u32 = WORKGROUP_SIZE / 32u;
const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;

var<workgroup> contributor_count: atomic<u32>;
var<workgroup> candidate_count: atomic<u32>;
var<workgroup> contributor_masks: array<atomic<u32>, 4>;

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

fn transform_covariance_terms_to_camera(
  c: CovTerms,
  r0: vec3<f32>,
  r1: vec3<f32>,
  r2: vec3<f32>,
) -> CovTerms {
  return CovTerms(
    covariance_quadratic(c, r0),
    covariance_bilinear(c, r0, r1),
    covariance_bilinear(c, r0, r2),
    covariance_quadratic(c, r1),
    covariance_bilinear(c, r1, r2),
    covariance_quadratic(c, r2),
  );
}

fn invalid_splat(candidate: u32) -> ProjectedSplat {
  return ProjectedSplat(
    vec2<f32>(2.0, 2.0),
    vec2<f32>(0.0),
    vec2<f32>(0.0),
    0.0,
    0u,
    candidate,
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

// Keep this predicate byte-for-byte equivalent to projected_quads_project.
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
  // Match the established CPU and direct-GPU source-order producer. In
  // particular, a non-finite depth must never enter the candidate prefix.
  // Alpha remains fail-open exactly like the qualified projection shader.
  if (!(p_cam.z >= params.near_plane && p_cam.z <= params.far_plane)
      || pa.w < ALPHA_THRESHOLD) {
    return invalid_splat(0u);
  }

  let f = 1.0 / tan(params.vertical_fov_radians * 0.5);
  let inv_z = 1.0 / p_cam.z;
  let x_ndc = (p_cam.x * f) * inv_z / params.aspect;
  let y_ndc = (p_cam.y * f) * inv_z;
  let covariance = CovTerms(cov0.x, cov0.y, cov0.z, cov0.w, cov1.x, cov1.y);
  let cov_cam = transform_covariance_terms_to_camera(covariance, r0, r1, r2);
  let tan_half_fovy = tan(params.vertical_fov_radians * 0.5);
  let tan_half_fovx = tan_half_fovy * params.aspect;
  let x_clamped = clamp(
    p_cam.x / p_cam.z,
    -1.3 * tan_half_fovx,
    1.3 * tan_half_fovx,
  ) * p_cam.z;
  let y_clamped = clamp(
    p_cam.y / p_cam.z,
    -1.3 * tan_half_fovy,
    1.3 * tan_half_fovy,
  ) * p_cam.z;
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
  var a = j00 * j00 * cov_cam.xx
    + 2.0 * j00 * j02 * cov_cam.xz
    + j02 * j02 * cov_cam.zz;
  let b = cov01;
  var c = j11 * j11 * cov_cam.yy
    + 2.0 * j11 * j12 * cov_cam.yz
    + j12 * j12 * cov_cam.zz;
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
    axis_u_dir = normalize2_or_default(
      vec2<f32>(b, major - a),
      vec2<f32>(1.0, 0.0),
    );
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
    return invalid_splat(1u);
  }
  return ProjectedSplat(
    vec2<f32>(x_ndc, y_ndc),
    axis_u,
    axis_v,
    pa.w,
    bitcast<u32>(max(p_cam.z, 0.0)),
    1u,
  );
}

@compute @workgroup_size(128)
fn project_count(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let group_count = (params.len + WORKGROUP_SIZE - 1u) / WORKGROUP_SIZE;
  if (group >= group_count) {
    return;
  }
  if (lane == 0u) {
    atomicStore(&candidate_count, 0u);
    atomicStore(&contributor_count, 0u);
  }
  workgroupBarrier();

  let source_id = group * WORKGROUP_SIZE + lane;
  var candidate = false;
  var contributes = false;
  if (source_id < params.len) {
    let projected = project_splat(source_id);
    projected_center_alpha_key[source_id] = vec4<f32>(
      projected.center,
      projected.alpha,
      bitcast<f32>(projected.key),
    );
    projected_axes[source_id] = vec2<u32>(pack2x16float(projected.axis_u), pack2x16float(projected.axis_v));
    candidate = projected.candidate != 0u;
    contributes = !(projected.alpha < ALPHA_THRESHOLD);
  }
  if (candidate) {
    atomicAdd(&candidate_count, 1u);
  }
  if (contributes) {
    atomicAdd(&contributor_count, 1u);
  }
  workgroupBarrier();
  if (lane == 0u) {
    atomicStore(
      &candidate_group_counts[group],
      atomicLoad(&candidate_count),
    );
    atomicStore(
      &contributor_group_counts[group],
      atomicLoad(&contributor_count),
    );
  }
}

@compute @workgroup_size(128)
fn compact_key_id(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let group_count =
    (compact_params.len + WORKGROUP_SIZE - 1u) / WORKGROUP_SIZE;
  if (group >= group_count) {
    return;
  }
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;
  if (lane < MASK_WORD_COUNT) {
    atomicStore(&contributor_masks[lane], 0u);
  }
  workgroupBarrier();

  let source_id = group * WORKGROUP_SIZE + lane;
  var contributes = false;
  var key = 0u;
  if (source_id < compact_params.len) {
    let center_alpha_key = compact_center_alpha_key[source_id];
    contributes = !(center_alpha_key.z < ALPHA_THRESHOLD);
    key = bitcast<u32>(center_alpha_key.w);
    if (contributes) {
      atomicOr(&contributor_masks[word], 1u << bit);
    }
  }
  workgroupBarrier();

  if (contributes) {
    var local_rank = 0u;
    for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
      local_rank += countOneBits(atomicLoad(&contributor_masks[earlier_word]));
    }
    local_rank += countOneBits(
      atomicLoad(&contributor_masks[word]) & lower_lane_mask,
    );
    let output_index = contributor_group_offsets[group] + local_rank;
    compact_keys[output_index] = key;
    compact_ids[output_index] = source_id;
  }
}

@compute @workgroup_size(1)
fn finalize_compaction() {
  let source_groups =
    (compact_params.len + WORKGROUP_SIZE - 1u) / WORKGROUP_SIZE;
  let count = contributor_group_offsets[source_groups];
  let active_groups = (count + SORT_TILE_SIZE - 1u) / SORT_TILE_SIZE;
  let limit = max(order_control.dispatch_limit, 1u);
  let dispatch_x = min(active_groups, limit);
  // Do not use `select` here: both operands may be evaluated and the empty
  // C=0 frame would then divide by zero. A zero indirect X dimension is a
  // valid no-op dispatch; Y stays one so the command remains well formed.
  var dispatch_y = 1u;
  if (dispatch_x > 0u) {
    dispatch_y = (active_groups + dispatch_x - 1u) / dispatch_x;
  }
  order_control.contributor_count = count;
  order_control.active_group_count = active_groups;
  order_control.dispatch_x = dispatch_x;
  order_control.dispatch_y = dispatch_y;
  order_control.dispatch_z = 1u;
  order_control.capacity_count = compact_params.len;
  atomicStore(&draw_args.instance_count, count);
}
