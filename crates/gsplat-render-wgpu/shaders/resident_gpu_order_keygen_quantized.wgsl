struct QuantizedSource {
  pos_xy: u32,
  pos_z_alpha: u32,
  rotation: u32,
  scale_rgb: u32,
  color_dc: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

struct SurfaceSourceElem {
  position: vec4<f32>,
  covariance0: vec4<f32>,
  covariance1: vec4<f32>,
  color_dc: vec4<f32>,
};

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
  _order_pad0: u32,
  _order_pad1: u32,
};

struct SortPair {
  key: u32,
  id: u32,
};

@group(0) @binding(0)
var<storage, read> key_source: array<QuantizedSource>;
@group(0) @binding(1)
var<uniform> render_params: RenderParams;
@group(0) @binding(2)
var<storage, read_write> generated_pairs: array<SortPair>;
@group(0) @binding(3)
var<storage, read_write> visibility_flags: array<u32>;

const WORKGROUP_SIZE: u32 = 64u;
const ITEMS_PER_THREAD: u32 = 4u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const SQRT_ONE_HALF: f32 = 0.7071067811865476;

fn decode_smallest_three(packed_in: u32) -> vec4<f32> {
  var packed = packed_in;
  let largest = packed >> 30u;
  var rotation = vec4<f32>(0.0);
  var sum_squares = 0.0;
  for (var step = 0u; step < 4u; step++) {
    let index = 3u - step;
    if (index == largest) {
      continue;
    }
    let magnitude = packed & 0x1ffu;
    let negative = (packed >> 9u) & 1u;
    packed = packed >> 10u;
    let value = SQRT_ONE_HALF * f32(magnitude) / 511.0;
    let signed_value = select(value, -value, negative != 0u);
    if (index == 0u) { rotation.x = signed_value; }
    else if (index == 1u) { rotation.y = signed_value; }
    else if (index == 2u) { rotation.z = signed_value; }
    else { rotation.w = signed_value; }
    sum_squares = sum_squares + value * value;
  }
  let largest_value = sqrt(max(1.0 - sum_squares, 0.0));
  if (largest == 0u) { rotation.x = largest_value; }
  else if (largest == 1u) { rotation.y = largest_value; }
  else if (largest == 2u) { rotation.z = largest_value; }
  else { rotation.w = largest_value; }
  return rotation;
}

fn quat_to_mat3(q: vec4<f32>) -> mat3x3<f32> {
  let n2 = dot(q, q);
  let qn = select(vec4<f32>(0.0, 0.0, 0.0, 1.0), q * inverseSqrt(n2), n2 > 0.0);
  let x = qn.x;
  let y = qn.y;
  let z = qn.z;
  let w = qn.w;
  let xx = x * x;
  let yy = y * y;
  let zz = z * z;
  let xy = x * y;
  let xz = x * z;
  let yz = y * z;
  let wx = w * x;
  let wy = w * y;
  let wz = w * z;
  return mat3x3<f32>(
    vec3<f32>(1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy)),
    vec3<f32>(2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx)),
    vec3<f32>(2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy)),
  );
}

fn dequant_source(elem: QuantizedSource) -> SurfaceSourceElem {
  let pos_xy = unpack2x16float(elem.pos_xy);
  let pos_za = unpack2x16float(elem.pos_z_alpha);
  let q = decode_smallest_three(elem.rotation);
  let scale_u = elem.scale_rgb;
  let log_s = vec3<f32>(
    f32(scale_u & 255u) / 16.0 - 10.0,
    f32((scale_u >> 8u) & 255u) / 16.0 - 10.0,
    f32((scale_u >> 16u) & 255u) / 16.0 - 10.0,
  );
  let s = exp(log_s);
  let s2 = max(s * s, vec3<f32>(1e-12));
  let r = quat_to_mat3(q);
  let rs = mat3x3<f32>(r[0] * s2.x, r[1] * s2.y, r[2] * s2.z);
  let world = rs * transpose(r);
  var source: SurfaceSourceElem;
  source.position = vec4<f32>(pos_xy.x, pos_xy.y, pos_za.x, 0.0);
  source.covariance0 = vec4<f32>(world[0][0], world[1][0], world[2][0], world[1][1]);
  source.covariance1 = vec4<f32>(world[2][1], world[2][2], pos_za.y, 0.0);
  source.color_dc = vec4<f32>(0.0);
  return source;
}

@compute @workgroup_size(64)
fn generate_pairs(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let first = group_id3.x * TILE_SIZE + local_id3.x * ITEMS_PER_THREAD;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    if (index < render_params.len) {
      let source = dequant_source(key_source[index]);
      let relative = source.position.xyz - render_params.camera_pos.xyz;
      let r0 = render_params.view_rot_row0.xyz;
      let r1 = render_params.view_rot_row1.xyz;
      let r2 = render_params.view_rot_row2.xyz;
      let p_cam = vec3<f32>(dot(r0, relative), dot(r1, relative), dot(r2, relative));
      var key = 0u;
      var visible = 0u;
      if (p_cam.z >= render_params.near_plane && p_cam.z <= render_params.far_plane && p_cam.z > 1e-6) {
        var keep = true;
        if (render_params.vertical_fov_radians > 0.0 &&
            render_params.width > 0u &&
            render_params.height > 0u) {
          keep = ndc_footprint_visible(source, p_cam);
        }
        if (keep) {
          // Positive finite IEEE-754 values have the same ordering as their bits.
          key = bitcast<u32>(max(p_cam.z, 0.0));
          visible = 1u;
        }
      }
      generated_pairs[index] = SortPair(key, index);
      visibility_flags[index] = visible;
    }
  }
}
