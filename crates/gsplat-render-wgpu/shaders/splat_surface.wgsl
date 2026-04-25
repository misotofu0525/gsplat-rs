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

fn eval_color(idx: u32, alpha: f32) -> vec4<f32> {
  let elem = color_elems[idx];
  let dir = normalize3_or_default(elem.position.xyz - params.camera_pos.xyz, vec3<f32>(0.0, 0.0, 1.0));
  let degree = min(params.sh_degree, 4u);
  let sh_rgb = vec3<f32>(
    eval_sh_channel(idx, 0u, elem.color_dc.x, dir, degree),
    eval_sh_channel(idx, 1u, elem.color_dc.y, dir, degree),
    eval_sh_channel(idx, 2u, elem.color_dc.z, dir, degree),
  );
  let rgb = clamp(sh_rgb + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
  return vec4<f32>(rgb * alpha, alpha);
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let instance = instances[instance_index];
  let quad = quad_offset(vertex_index);
  let center = instance.center_and_axis_u.xy;
  let axis_u = instance.center_and_axis_u.zw;
  let axis_v = instance.axis_v_index_alpha.xy;
  let offset = axis_u * quad.x + axis_v * quad.y;
  let source_idx = u32(instance.axis_v_index_alpha.z + 0.5);

  var out: VsOut;
  out.position = vec4<f32>(center + offset, 0.0, 1.0);
  out.color = eval_color(source_idx, instance.axis_v_index_alpha.w);
  out.local = quad;
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
