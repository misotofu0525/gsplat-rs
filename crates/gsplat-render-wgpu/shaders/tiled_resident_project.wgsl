struct RenderParams {
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

struct TiledParams {
  width: u32,
  height: u32,
  tiles_x: u32,
  tiles_y: u32,
  source_count: u32,
  work_count: u32,
  tile_count: u32,
  entry_capacity: u32,
  entry_count: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
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
var<storage, read> position_alpha: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> covariance0: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> covariance1: array<vec2<f32>>;
@group(0) @binding(3)
var<uniform> params: RenderParams;
@group(0) @binding(4)
var<storage, read_write> projected_center: array<vec4<f32>>;
@group(0) @binding(5)
var<storage, read_write> projected_conic: array<vec4<f32>>;
@group(0) @binding(6)
var<storage, read_write> projected_bbox: array<vec4<u32>>;
@group(0) @binding(7)
var<uniform> tiled_params: TiledParams;

const WORKGROUP_SIZE: u32 = 128u;
const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn canonical_dot3(left: vec3<f32>, right: vec3<f32>) -> f32 {
  let xy = fma(left.y, right.y, left.x * right.x);
  return fma(left.z, right.z, xy);
}

fn is_finite(value: f32) -> bool {
  return (bitcast<u32>(value) & 0x7f800000u) != 0x7f800000u;
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
  let cb = vec3<f32>(
    c.xx * b.x + c.xy * b.y + c.xz * b.z,
    c.xy * b.x + c.yy * b.y + c.yz * b.z,
    c.xz * b.x + c.yz * b.y + c.zz * b.z,
  );
  return dot(a, cb);
}

fn transform_covariance(c: CovTerms, r0: vec3<f32>, r1: vec3<f32>, r2: vec3<f32>) -> CovTerms {
  return CovTerms(
    covariance_quadratic(c, r0),
    covariance_bilinear(c, r0, r1),
    covariance_bilinear(c, r0, r2),
    covariance_quadratic(c, r1),
    covariance_bilinear(c, r1, r2),
    covariance_quadratic(c, r2),
  );
}

fn invalidate(index: u32, depth: f32, opacity: f32) {
  projected_center[index] = vec4<f32>(0.0, 0.0, depth, opacity);
  projected_conic[index] = vec4<f32>(0.0);
  projected_bbox[index] = vec4<u32>(0u);
}

@compute @workgroup_size(128)
fn project_resident(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let index = logical_workgroup_id(wg_id, num_wg) * WORKGROUP_SIZE + lane;
  // CPU ordering can contain any source ID while drawing only its visible
  // subset. Projection therefore covers the resident source capacity, not the
  // draw-instance count stored in RenderParams.len.
  if (index >= tiled_params.source_count) {
    return;
  }
  let pa = position_alpha[index];
  let rel = pa.xyz - params.camera_pos.xyz;
  let r0 = params.view_rot_row0.xyz;
  let r1 = params.view_rot_row1.xyz;
  let r2 = params.view_rot_row2.xyz;
  let p_cam = vec3<f32>(
    canonical_dot3(r0, rel),
    canonical_dot3(r1, rel),
    canonical_dot3(r2, rel),
  );
  if (p_cam.z < params.near_plane || p_cam.z > params.far_plane || pa.w < ALPHA_THRESHOLD) {
    invalidate(index, p_cam.z, pa.w);
    return;
  }

  let tan_half_fovy = tan(params.vertical_fov_radians * 0.5);
  let fy = 1.0 / tan_half_fovy;
  let fx = fy / params.aspect;
  let inv_z = 1.0 / p_cam.z;
  let x_ndc = p_cam.x * fx * inv_z;
  let y_ndc = p_cam.y * fy * inv_z;
  let center = vec2<f32>(
    (x_ndc * 0.5 + 0.5) * f32(params.width),
    (0.5 - y_ndc * 0.5) * f32(params.height),
  );

  let cov0 = covariance0[index];
  let cov1 = covariance1[index];
  let world_cov = CovTerms(cov0.x, cov0.y, cov0.z, cov0.w, cov1.x, cov1.y);
  let cov = transform_covariance(world_cov, r0, r1, r2);
  let x_clamped = clamp(
    p_cam.x * inv_z,
    -1.3 * tan_half_fovy * params.aspect,
     1.3 * tan_half_fovy * params.aspect,
  ) * p_cam.z;
  let y_clamped = clamp(
    p_cam.y * inv_z,
    -1.3 * tan_half_fovy,
     1.3 * tan_half_fovy,
  ) * p_cam.z;
  let inv_z2 = inv_z * inv_z;
  let j00 = fx * inv_z;
  let j02 = -fx * x_clamped * inv_z2;
  let j11 = fy * inv_z;
  let j12 = -fy * y_clamped * inv_z2;
  let cov_ndc_xy = j00 * j11 * cov.xy
    + j00 * j12 * cov.xz
    + j02 * j11 * cov.yz
    + j02 * j12 * cov.zz;
  let cov_ndc_xx = j00 * j00 * cov.xx
    + 2.0 * j00 * j02 * cov.xz
    + j02 * j02 * cov.zz;
  let cov_ndc_yy = j11 * j11 * cov.yy
    + 2.0 * j11 * j12 * cov.yz
    + j12 * j12 * cov.zz;
  let half_width = f32(params.width) * 0.5;
  let half_height = f32(params.height) * 0.5;
  let cov_xx = cov_ndc_xx * half_width * half_width + 0.3;
  let cov_xy = -cov_ndc_xy * half_width * half_height;
  let cov_yy = cov_ndc_yy * half_height * half_height + 0.3;
  let determinant = cov_xx * cov_yy - cov_xy * cov_xy;
  if (!(determinant > 0.0)
      || !is_finite(determinant)
      || !is_finite(center.x)
      || !is_finite(center.y)
      || !is_finite(cov_xx)
      || !is_finite(cov_yy)) {
    invalidate(index, p_cam.z, pa.w);
    return;
  }
  let conic = vec3<f32>(cov_yy, -cov_xy, cov_xx) / determinant;
  let mid = 0.5 * (cov_xx + cov_yy);
  let half_difference = 0.5 * (cov_xx - cov_yy);
  let max_eigenvalue = max(mid + sqrt(max(half_difference * half_difference + cov_xy * cov_xy, 0.0)), 0.0);
  let radius = ceil(3.0 * sqrt(max_eigenvalue));
  let min_f = floor(center - vec2<f32>(radius));
  let max_f = ceil(center + vec2<f32>(radius));
  let min_xy = vec2<u32>(clamp(min_f, vec2<f32>(0.0), vec2<f32>(f32(params.width), f32(params.height))));
  let max_xy = vec2<u32>(clamp(max_f, vec2<f32>(0.0), vec2<f32>(f32(params.width), f32(params.height))));
  if (any(min_xy >= max_xy)) {
    invalidate(index, p_cam.z, pa.w);
    return;
  }
  projected_center[index] = vec4<f32>(center, p_cam.z, pa.w);
  // q <= 9 is the existing three-sigma support. Low-opacity splats have a
  // smaller exact 1/255 contribution support; retaining that q limit lets
  // tile work generation reject only tiles that cannot contain a surviving
  // pixel, without probing every pixel in the tile.
  let contribution_q_limit = min(9.0, max(2.0 * log(pa.w * 255.0), 0.0));
  projected_conic[index] = vec4<f32>(conic, contribution_q_limit);
  projected_bbox[index] = vec4<u32>(min_xy, max_xy);
}
