struct InstanceData {
  // xy = center in NDC, zw = major axis in NDC
  center_and_axis_u: vec4<f32>,
  // xy = minor axis in NDC, z = source splat index, w = alpha
  axis_v_index_alpha: vec4<f32>,
};

struct SurfaceColorElem {
  position: vec4<f32>,
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

@group(0) @binding(0)
var<storage, read> instances: array<InstanceData>;
@group(0) @binding(1)
var<storage, read> color_elems: array<SurfaceColorElem>;
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

fn normalize3_or_default(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
  let len2 = dot(v, v);
  if (len2 <= 1e-20) {
    return fallback;
  }
  return v * inverseSqrt(len2);
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

fn eval_color(idx: u32, alpha: f32) -> vec4<f32> {
  let elem = color_elems[idx];
  let dir = normalize3_or_default(elem.position.xyz - params.camera_pos.xyz, vec3<f32>(0.0, 0.0, 1.0));
  let degree = min(params.sh_degree, 4u);
  let sh_rgb = eval_sh_rgb(idx, elem.color_dc.xyz, dir, degree);
  let rgb = clamp(sh_rgb + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
  return vec4<f32>(rgb * alpha, alpha);
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let instance = instances[instance_index];
  let center = instance.center_and_axis_u.xy;
  let axis_u = instance.center_and_axis_u.zw;
  let axis_v = instance.axis_v_index_alpha.xy;
  let local = quad_offset(vertex_index) * alpha_extent_scale(instance.axis_v_index_alpha.w);
  let offset = axis_u * local.x + axis_v * local.y;
  let source_idx = u32(instance.axis_v_index_alpha.z + 0.5);

  var out: VsOut;
  out.position = vec4<f32>(center + offset, 0.0, 1.0);
  out.color = eval_color(source_idx, instance.axis_v_index_alpha.w);
  out.local = local;
  return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
  let r2 = dot(input.local, input.local);
  if (r2 > 1.0) {
    discard;
  }

  // Axis vectors encode a 3-sigma ellipse, so local-space exponent scales by 9.
  let g = exp(-4.5 * r2);
  let alpha = input.color.a * g;
  if (alpha <= (1.0 / 256.0)) {
    discard;
  }
  return vec4<f32>(input.color.rgb * g, alpha);
}
